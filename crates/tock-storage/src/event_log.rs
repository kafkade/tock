//! SQLite-backed append-only event log with per-entity AEAD.
//!
//! Architecture §3.2 (`events` table schema) + §6.1 (event semantics) +
//! issue #5 (Ed25519 signature column).
//!
//! ## Append flow
//!
//! 1. Caller provides the plaintext payload and the event metadata.
//! 2. Derive per-entity item key `IK = HKDF(VK, info="tock/v1/item/" || kind || entity_id)`.
//! 3. Encrypt payload with AES-256-GCM under `IK`; AAD = canonical
//!    binding tuple of `entity_kind`, `entity_id`, `op`, `lamport`, `device_id`.
//! 4. Sign the full event (including ciphertext, nonce, AAD, vector
//!    clock, …) with the local device's Ed25519 signing key.
//! 5. Insert the row.
//!
//! ## Read flow
//!
//! Stream rows, reconstruct each `SignedEvent`, verify signature
//! against the device registry, then AEAD-decrypt the payload. Any
//! integrity failure surfaces as [`Error::EventLogIntegrity`] or
//! [`Error::InvalidVaultOrCredentials`] (signature verify failures use
//! the latter so they're indistinguishable from header tampering).

use rusqlite::{Connection, OptionalExtension, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tock_core::event::{
    DeviceId, EntityKind, Event, EventOp, SignedEvent, VectorClock, canonical_signing_bytes, sign,
    verify,
};
use tock_core::vault::KeyHierarchy;
use tock_crypto::aead::{self, Key as AeadKey, Nonce as AeadNonce};
use tock_crypto::signature::{Signature, VerifyingKey};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::Error;
use crate::vault::OpenVault;

/// Read result: the verified, signed event plus its decrypted (and
/// zeroize-on-drop) payload.
pub type VerifiedEvent = (SignedEvent, Zeroizing<Vec<u8>>);

/// Borrowed view of the event log against an open vault.
pub struct EventLog<'a> {
    vault: &'a OpenVault,
}

impl<'a> EventLog<'a> {
    /// Borrow the event log of an open vault.
    #[must_use]
    pub const fn new(vault: &'a OpenVault) -> Self {
        Self { vault }
    }

    /// Append a new event. The plaintext payload is encrypted before
    /// being persisted; the full event is signed by the vault's
    /// local-device signing key.
    ///
    /// Caller supplies `(entity_kind, entity_id, op, plaintext)` plus
    /// the vector-clock context; the lamport counter is the
    /// next-after-max for the local device.
    ///
    /// # Errors
    /// - [`Error::Crypto`] on RNG / AEAD failure.
    /// - [`Error::Sqlite`] on persistence failure.
    pub fn append(
        &self,
        entity_kind: EntityKind,
        entity_id: Uuid,
        op: EventOp,
        plaintext: &[u8],
        vector_clock: VectorClock,
        parent_event_id: Option<Uuid>,
    ) -> Result<SignedEvent, Error> {
        let device = self.vault.local_device();
        let device_id = DeviceId::from_bytes(device.device_id);
        let next_lamport = self.next_local_lamport()?;
        let _span = tracing::info_span!(
            "event_log::append",
            entity_kind = entity_kind.as_str(),
            entity_id = %entity_id,
            lamport = next_lamport,
        )
        .entered();

        let id = Uuid::now_v7();
        let now = OffsetDateTime::now_utc();

        // Derive IK and seal payload.
        let dk = KeyHierarchy::derive_domain_key(self.vault.vault_key(), entity_kind.as_str())?;
        let ik = KeyHierarchy::derive_item_key(&dk, entity_id.as_bytes())?;
        let ik_key = AeadKey::from_secret(ik);
        let payload_nonce_obj = AeadNonce::try_random()?;
        let payload_nonce = *payload_nonce_obj.as_bytes();
        let payload_aad = build_aad(entity_kind, entity_id, &op, next_lamport, device_id);
        let payload_ct = aead::seal(&ik_key, &payload_nonce_obj, &payload_aad, plaintext)?;

        let event = Event {
            id,
            device_id,
            lamport: next_lamport,
            vector_clock,
            parent_event_id,
            entity_kind,
            entity_id,
            op,
            payload_ct,
            payload_nonce,
            payload_aad,
            created_at: now,
        };
        let signed = sign(event, &device.signing_key);

        insert_signed_event(self.vault.connection(), &signed)?;
        tracing::debug!(event_id = %signed.event.id, "event appended");
        Ok(signed)
    }

    /// Iterate all events in `(lamport ASC, device_id ASC)` order.
    /// Each returned `(SignedEvent, plaintext)` has had its signature
    /// verified against the device registry and its payload AEAD
    /// authenticated.
    ///
    /// # Errors
    /// Streams `Result` per event; first error stops iteration.
    pub fn read_all(&self) -> Result<Vec<VerifiedEvent>, Error> {
        let conn = self.vault.connection();
        let mut stmt = conn.prepare(
            "SELECT id, device_id, lamport, vector_clock, parent_event_id,
                    entity_kind, entity_id, op_tag, op_sub_tag,
                    payload_ct, payload_nonce, payload_aad,
                    signature, signer, created_at
             FROM events
             ORDER BY lamport ASC, device_id ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let signed = row_to_signed_event(row)?;
            verify(&signed, |id| lookup_device_key(conn, id).ok().flatten())
                .map_err(|_| Error::InvalidVaultOrCredentials)?;
            // AEAD-open the payload.
            let dk = KeyHierarchy::derive_domain_key(
                self.vault.vault_key(),
                signed.event.entity_kind.as_str(),
            )?;
            let ik = KeyHierarchy::derive_item_key(&dk, signed.event.entity_id.as_bytes())?;
            let ik_key = AeadKey::from_secret(ik);
            let nonce = AeadNonce::from_bytes(signed.event.payload_nonce);
            let pt = aead::open(
                &ik_key,
                &nonce,
                &signed.event.payload_aad,
                &signed.event.payload_ct,
            )
            .map_err(|_| Error::EventLogIntegrity)?;
            out.push((signed, pt));
        }
        Ok(out)
    }

    /// Lookup the largest lamport value stored for the local device,
    /// plus one. Used to allocate the next outgoing event's clock.
    fn next_local_lamport(&self) -> Result<u64, Error> {
        let conn = self.vault.connection();
        let max: Option<i64> = conn
            .query_row(
                "SELECT MAX(lamport) FROM events WHERE device_id = ?1",
                params![self.vault.local_device().device_id.to_vec()],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let max_u: u64 = max.unwrap_or(0).max(0).try_into().unwrap_or(0);
        Ok(max_u + 1)
    }

    /// Highest lamport stored for the local device (0 if none). Used by
    /// the sync layer to allocate a contiguous run of outgoing lamports.
    pub(crate) fn local_lamport_high(&self) -> Result<u64, Error> {
        Ok(self.next_local_lamport()? - 1)
    }

    /// AEAD-open the payload of an already-reconstructed signed event,
    /// returning the zeroize-on-drop plaintext. Used when ingesting a
    /// remote event whose signature has been verified.
    ///
    /// # Errors
    /// [`Error::EventLogIntegrity`] if the payload fails authentication.
    pub(crate) fn decrypt_payload(
        &self,
        signed: &SignedEvent,
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
        let dk = KeyHierarchy::derive_domain_key(
            self.vault.vault_key(),
            signed.event.entity_kind.as_str(),
        )?;
        let ik = KeyHierarchy::derive_item_key(&dk, signed.event.entity_id.as_bytes())?;
        let ik_key = AeadKey::from_secret(ik);
        let nonce = AeadNonce::from_bytes(signed.event.payload_nonce);
        aead::open(
            &ik_key,
            &nonce,
            &signed.event.payload_aad,
            &signed.event.payload_ct,
        )
        .map_err(|_| Error::EventLogIntegrity)
    }
}

/// Insert a (foreign) signed event into the local log. Crate-internal
/// hook for the sync ingest pipeline.
///
/// # Errors
/// [`Error::Sqlite`] on persistence failure.
pub(crate) fn insert_event(conn: &Connection, signed: &SignedEvent) -> Result<(), Error> {
    insert_signed_event(conn, signed)
}

/// Whether an event id already exists in the local log.
///
/// # Errors
/// [`Error::Sqlite`] on query failure.
pub(crate) fn event_exists(conn: &Connection, id: Uuid) -> Result<bool, Error> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM events WHERE id = ?1",
            params![id.as_bytes().to_vec()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

/// Load the metadata (not payload plaintext) of an event by id.
///
/// # Errors
/// [`Error::Sqlite`] or [`Error::EventLogIntegrity`] on decode failure.
pub(crate) fn load_event_meta(conn: &Connection, id: Uuid) -> Result<Option<Event>, Error> {
    let mut stmt = conn.prepare(
        "SELECT id, device_id, lamport, vector_clock, parent_event_id,
                entity_kind, entity_id, op_tag, op_sub_tag,
                payload_ct, payload_nonce, payload_aad,
                signature, signer, created_at
         FROM events WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id.as_bytes().to_vec()])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_signed_event(row)?.event)),
        None => Ok(None),
    }
}

/// Build the canonical event-payload AAD (architecture §5.3).
fn build_aad(
    kind: EntityKind,
    entity_id: Uuid,
    op: &EventOp,
    lamport: u64,
    device: DeviceId,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(b"tock|v1|");
    out.extend_from_slice(kind.as_str().as_bytes());
    out.push(b'|');
    out.extend_from_slice(entity_id.as_bytes());
    out.push(b'|');
    out.extend_from_slice(op.tag().as_bytes());
    out.push(b'|');
    out.extend_from_slice(&lamport.to_be_bytes());
    out.push(b'|');
    out.extend_from_slice(device.as_bytes());
    out
}

fn insert_signed_event(conn: &Connection, signed: &SignedEvent) -> Result<(), Error> {
    let vc_bytes = encode_vector_clock(&signed.event.vector_clock);
    let parent = signed.event.parent_event_id.map(|u| u.as_bytes().to_vec());
    let entity_id = signed.event.entity_id.as_bytes().to_vec();
    let id_bytes = signed.event.id.as_bytes().to_vec();
    let device_id = signed.event.device_id.as_bytes().to_vec();
    let lamport_i: i64 = i64::try_from(signed.event.lamport).unwrap_or(i64::MAX);
    let created = signed.event.created_at.format(&Rfc3339).unwrap_or_default();
    conn.execute(
        "INSERT INTO events
         (id, device_id, lamport, vector_clock, parent_event_id,
          entity_kind, entity_id, op_tag, op_sub_tag,
          payload_ct, payload_nonce, payload_aad,
          signature, signer, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        params![
            id_bytes,
            device_id,
            lamport_i,
            vc_bytes,
            parent,
            signed.event.entity_kind.as_str(),
            entity_id,
            signed.event.op.tag(),
            signed.event.op.sub_tag_bytes(),
            signed.event.payload_ct,
            signed.event.payload_nonce.to_vec(),
            signed.event.payload_aad,
            signed.signature.to_bytes().to_vec(),
            signed.signer.to_bytes().to_vec(),
            created,
        ],
    )?;
    Ok(())
}

fn row_to_signed_event(row: &rusqlite::Row<'_>) -> Result<SignedEvent, Error> {
    let id: Vec<u8> = row.get(0)?;
    let device_id: Vec<u8> = row.get(1)?;
    let lamport_i: i64 = row.get(2)?;
    let vc: Vec<u8> = row.get(3)?;
    let parent: Option<Vec<u8>> = row.get(4)?;
    let kind: String = row.get(5)?;
    let entity_id: Vec<u8> = row.get(6)?;
    let op_tag: String = row.get(7)?;
    let op_sub: Vec<u8> = row.get(8)?;
    let payload_ct: Vec<u8> = row.get(9)?;
    let payload_nonce: Vec<u8> = row.get(10)?;
    let payload_aad: Vec<u8> = row.get(11)?;
    let signature: Vec<u8> = row.get(12)?;
    let signer: Vec<u8> = row.get(13)?;
    let created: String = row.get(14)?;

    let id_arr: [u8; 16] = id
        .as_slice()
        .try_into()
        .map_err(|_| Error::EventLogIntegrity)?;
    let device_arr: [u8; 16] = device_id
        .as_slice()
        .try_into()
        .map_err(|_| Error::EventLogIntegrity)?;
    let entity_arr: [u8; 16] = entity_id
        .as_slice()
        .try_into()
        .map_err(|_| Error::EventLogIntegrity)?;
    let nonce_arr: [u8; 12] = payload_nonce
        .as_slice()
        .try_into()
        .map_err(|_| Error::EventLogIntegrity)?;
    let signature_arr: [u8; 64] = signature
        .as_slice()
        .try_into()
        .map_err(|_| Error::EventLogIntegrity)?;
    let signer_arr: [u8; 32] = signer
        .as_slice()
        .try_into()
        .map_err(|_| Error::EventLogIntegrity)?;
    let parent_uuid = match parent {
        Some(p) => Some(Uuid::from_bytes(
            p.as_slice()
                .try_into()
                .map_err(|_| Error::EventLogIntegrity)?,
        )),
        None => None,
    };

    let event = Event {
        id: Uuid::from_bytes(id_arr),
        device_id: DeviceId::from_bytes(device_arr),
        lamport: u64::try_from(lamport_i.max(0)).unwrap_or(0),
        vector_clock: decode_vector_clock(&vc)?,
        parent_event_id: parent_uuid,
        entity_kind: parse_kind(&kind)?,
        entity_id: Uuid::from_bytes(entity_arr),
        op: parse_op(&op_tag, &op_sub)?,
        payload_ct,
        payload_nonce: nonce_arr,
        payload_aad,
        created_at: OffsetDateTime::parse(&created, &Rfc3339)
            .map_err(|_| Error::EventLogIntegrity)?,
    };
    // Force the canonical bytes to be computable (sanity).
    let _ = canonical_signing_bytes(
        &event,
        &VerifyingKey::from_bytes(&signer_arr).map_err(|_| Error::EventLogIntegrity)?,
    );
    let signer_vk = VerifyingKey::from_bytes(&signer_arr).map_err(|_| Error::EventLogIntegrity)?;
    let sig = Signature::from_bytes(&signature_arr);
    Ok(SignedEvent {
        event,
        signature: sig,
        signer: signer_vk,
    })
}

fn lookup_device_key(conn: &Connection, id: &DeviceId) -> Result<Option<VerifyingKey>, Error> {
    let bytes: Option<Vec<u8>> = conn
        .query_row(
            "SELECT verifying_key FROM devices WHERE device_id = ?1",
            params![id.as_bytes().to_vec()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match bytes {
        Some(b) => {
            let arr: [u8; 32] = b
                .as_slice()
                .try_into()
                .map_err(|_| Error::EventLogIntegrity)?;
            Some(VerifyingKey::from_bytes(&arr).map_err(|_| Error::EventLogIntegrity)?)
        }
        None => None,
    })
}

fn encode_vector_clock(vc: &VectorClock) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + vc.0.len() * 24);
    let n = u32::try_from(vc.0.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&n.to_le_bytes());
    for (dev, lam) in &vc.0 {
        out.extend_from_slice(dev.as_bytes());
        out.extend_from_slice(&lam.to_le_bytes());
    }
    out
}

fn decode_vector_clock(bytes: &[u8]) -> Result<VectorClock, Error> {
    if bytes.len() < 4 {
        return Err(Error::EventLogIntegrity);
    }
    let n = u32::from_le_bytes(
        bytes[..4]
            .try_into()
            .map_err(|_| Error::EventLogIntegrity)?,
    ) as usize;
    let body = &bytes[4..];
    if body.len() != n * 24 {
        return Err(Error::EventLogIntegrity);
    }
    let mut map = std::collections::BTreeMap::new();
    for chunk in body.chunks_exact(24) {
        let dev: [u8; 16] = chunk[..16]
            .try_into()
            .map_err(|_| Error::EventLogIntegrity)?;
        let lam_bytes: [u8; 8] = chunk[16..24]
            .try_into()
            .map_err(|_| Error::EventLogIntegrity)?;
        map.insert(DeviceId::from_bytes(dev), u64::from_le_bytes(lam_bytes));
    }
    Ok(VectorClock(map))
}

fn parse_kind(s: &str) -> Result<EntityKind, Error> {
    Ok(match s {
        "task" => EntityKind::Task,
        "project" => EntityKind::Project,
        "heading" => EntityKind::Heading,
        "area" => EntityKind::Area,
        "habit" => EntityKind::Habit,
        "habit_entry" => EntityKind::HabitEntry,
        "time_block" => EntityKind::TimeBlock,
        "focus_session" => EntityKind::FocusSession,
        "annotation" => EntityKind::Annotation,
        "device" => EntityKind::Device,
        "tag" => EntityKind::Tag,
        "tag_link" => EntityKind::TagLink,
        "habit_skip" => EntityKind::HabitSkip,
        _ => return Err(Error::EventLogIntegrity),
    })
}

fn parse_op(tag: &str, sub: &[u8]) -> Result<EventOp, Error> {
    Ok(match tag {
        "create" => EventOp::Create,
        "delete" => EventOp::Delete,
        "purge" => EventOp::Purge,
        "snapshot" => EventOp::Snapshot,
        "append" => EventOp::Append {
            sub_kind: String::from_utf8(sub.to_vec()).map_err(|_| Error::EventLogIntegrity)?,
        },
        "update" => {
            let mut fields = Vec::new();
            let mut cursor = sub;
            while cursor.len() >= 4 {
                let n = u32::from_le_bytes(
                    cursor[..4]
                        .try_into()
                        .map_err(|_| Error::EventLogIntegrity)?,
                ) as usize;
                cursor = &cursor[4..];
                if cursor.len() < n {
                    return Err(Error::EventLogIntegrity);
                }
                fields.push(
                    String::from_utf8(cursor[..n].to_vec())
                        .map_err(|_| Error::EventLogIntegrity)?,
                );
                cursor = &cursor[n..];
            }
            if !cursor.is_empty() {
                return Err(Error::EventLogIntegrity);
            }
            EventOp::Update { fields }
        }
        _ => return Err(Error::EventLogIntegrity),
    })
}
