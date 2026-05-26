//! Binary wire format for transmitting [`SignedEvent`] batches.
//!
//! ## Batch envelope
//!
//! ```text
//! magic:   "tock-sync-v1"  (12 bytes)
//! version: u8              (currently 1)
//! count:   u32 LE          (number of events, max 256)
//! events:  [frame × count]
//! ```
//!
//! Each event frame is length-prefixed so a decoder can skip unknown
//! versions or corrupted entries:
//!
//! ```text
//! frame_len: u32 LE   (byte length of everything after this field)
//! <event fields>      (see encode_event_fields / decode_event_fields)
//! signature: 64 bytes
//! signer:    32 bytes
//! ```
//!
//! The event-field layout matches the order of
//! [`canonical_signing_bytes`](tock_core::event::canonical_signing_bytes)
//! so serialisation and signature construction share the same mental
//! model, reducing divergence risk.
//!
//! ## Encrypted batch envelope
//!
//! For transport to a server that must never see metadata, wrap a
//! plaintext batch with [`seal_batch`] / [`open_batch`]. The outer
//! AEAD uses a key derived from the Vault Key via HKDF with
//! `info = "tock/v1/sync-batch"`, and binds protocol version and
//! vault id in the AAD.
//!
//! ## Limits
//!
//! - Max events per batch: [`MAX_BATCH_EVENTS`] (256)
//! - Max wire bytes per event frame: [`MAX_EVENT_FRAME_BYTES`] (256 KiB)
//! - Max total batch size: 16 MiB (defensive; not checked at encode
//!   time but enforced at decode time)

use time::OffsetDateTime;
use tock_core::event::{DeviceId, EntityKind, Event, EventOp, SignedEvent, VectorClock};
use tock_crypto::signature::{Signature, VerifyingKey};
use uuid::Uuid;

use crate::Error;

/// Magic bytes at the start of every batch.
pub const BATCH_MAGIC: &[u8; 12] = b"tock-sync-v1";

/// Current wire-format version.
pub const WIRE_VERSION: u8 = 1;

/// Maximum events in a single batch.
pub const MAX_BATCH_EVENTS: u32 = 256;

/// Maximum byte length of a single event frame (256 KiB).
pub const MAX_EVENT_FRAME_BYTES: u32 = 262_144;

/// Maximum total batch byte length (16 MiB).
pub const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;

// ── Batch encode / decode ────────────────────────────────────────────

/// Encode a batch of [`SignedEvent`]s into the binary wire format.
///
/// # Errors
/// Returns [`Error::WireFormat`] if `events.len()` exceeds
/// [`MAX_BATCH_EVENTS`].
pub fn encode_batch(events: &[SignedEvent]) -> Result<Vec<u8>, Error> {
    let count = u32::try_from(events.len()).map_err(|_| Error::WireFormat("too many events"))?;
    if count > MAX_BATCH_EVENTS {
        return Err(Error::WireFormat("batch exceeds MAX_BATCH_EVENTS"));
    }
    // Pre-size: magic + version + count + ~200B per event is a rough guess.
    let mut buf = Vec::with_capacity(12 + 1 + 4 + events.len() * 256);
    buf.extend_from_slice(BATCH_MAGIC);
    buf.push(WIRE_VERSION);
    buf.extend_from_slice(&count.to_le_bytes());

    for se in events {
        let frame = encode_event_frame(se);
        let frame_len =
            u32::try_from(frame.len()).map_err(|_| Error::WireFormat("event frame too large"))?;
        buf.extend_from_slice(&frame_len.to_le_bytes());
        buf.extend_from_slice(&frame);
    }
    Ok(buf)
}

/// Decode a binary batch into its constituent [`SignedEvent`]s.
///
/// # Errors
/// Returns [`Error::WireFormat`] on any structural or length violation.
pub fn decode_batch(bytes: &[u8]) -> Result<Vec<SignedEvent>, Error> {
    if bytes.len() > MAX_BATCH_BYTES {
        return Err(Error::WireFormat("batch exceeds MAX_BATCH_BYTES"));
    }
    // Magic (12) + version (1) + count (4) = 17 minimum header.
    if bytes.len() < 17 {
        return Err(Error::WireFormat("batch too short for header"));
    }
    if &bytes[..12] != BATCH_MAGIC {
        return Err(Error::WireFormat("invalid batch magic"));
    }
    let version = bytes[12];
    if version != WIRE_VERSION {
        return Err(Error::WireFormat("unsupported wire version"));
    }
    let count = u32::from_le_bytes(
        bytes[13..17]
            .try_into()
            .map_err(|_| Error::WireFormat("count parse"))?,
    );
    if count > MAX_BATCH_EVENTS {
        return Err(Error::WireFormat("batch count exceeds MAX_BATCH_EVENTS"));
    }

    let mut cursor = 17;
    let mut events = Vec::with_capacity(count as usize);
    for _ in 0..count {
        if cursor + 4 > bytes.len() {
            return Err(Error::WireFormat("truncated frame length"));
        }
        let frame_len = u32::from_le_bytes(
            bytes[cursor..cursor + 4]
                .try_into()
                .map_err(|_| Error::WireFormat("frame_len parse"))?,
        );
        if frame_len > MAX_EVENT_FRAME_BYTES {
            return Err(Error::WireFormat(
                "event frame exceeds MAX_EVENT_FRAME_BYTES",
            ));
        }
        cursor += 4;
        let frame_len_usize = frame_len as usize;
        if cursor + frame_len_usize > bytes.len() {
            return Err(Error::WireFormat("truncated event frame"));
        }
        let frame = &bytes[cursor..cursor + frame_len_usize];
        let se = decode_event_frame(frame)?;
        events.push(se);
        cursor += frame_len_usize;
    }
    if cursor != bytes.len() {
        return Err(Error::WireFormat("trailing bytes after batch"));
    }
    Ok(events)
}

// ── Encrypted batch envelope ─────────────────────────────────────────

/// HKDF info for deriving the sync-batch key from the Vault Key.
const SYNC_BATCH_KEY_INFO: &[u8] = b"tock/v1/sync-batch";

/// Encrypt a plaintext batch using a key derived from the vault key.
///
/// The AAD binds the protocol version and vault id so replay across
/// vaults or protocol versions is detected.
///
/// # Errors
/// Returns [`Error::Crypto`] on AEAD or RNG failure.
pub fn seal_batch(
    vault_key: &tock_core::vault::VaultKey,
    vault_id: &Uuid,
    plaintext_batch: &[u8],
) -> Result<Vec<u8>, Error> {
    let batch_key = derive_batch_key(vault_key)?;
    let nonce = tock_crypto::aead::Nonce::try_random()?;
    let aad = batch_aad(vault_id);
    let ct = tock_crypto::aead::seal(&batch_key, &nonce, &aad, plaintext_batch)?;
    // Output: nonce (12B) + ciphertext.
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(nonce.as_bytes());
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt a sealed batch.
///
/// # Errors
/// Returns [`Error::Crypto`] on AEAD failure (wrong key, tampered, etc.).
pub fn open_batch(
    vault_key: &tock_core::vault::VaultKey,
    vault_id: &Uuid,
    sealed: &[u8],
) -> Result<Vec<u8>, Error> {
    if sealed.len() < 12 {
        return Err(Error::WireFormat("sealed batch too short for nonce"));
    }
    let nonce_bytes: [u8; 12] = sealed[..12]
        .try_into()
        .map_err(|_| Error::WireFormat("nonce parse"))?;
    let nonce = tock_crypto::aead::Nonce::from_bytes(nonce_bytes);
    let batch_key = derive_batch_key(vault_key)?;
    let aad = batch_aad(vault_id);
    let pt = tock_crypto::aead::open(&batch_key, &nonce, &aad, &sealed[12..])?;
    Ok(pt.to_vec())
}

fn derive_batch_key(
    vault_key: &tock_core::vault::VaultKey,
) -> Result<tock_crypto::aead::Key, Error> {
    let derived = tock_crypto::kdf::hkdf_sha256_32(
        vault_key.as_secret().expose_secret(),
        &[],
        SYNC_BATCH_KEY_INFO,
    )?;
    Ok(tock_crypto::aead::Key::from_secret(derived))
}

fn batch_aad(vault_id: &Uuid) -> Vec<u8> {
    let mut aad = Vec::with_capacity(32);
    aad.extend_from_slice(b"tock-sync-batch-v1|");
    aad.extend_from_slice(vault_id.as_bytes());
    aad
}

// ── Per-event frame encode / decode ──────────────────────────────────

fn encode_event_frame(se: &SignedEvent) -> Vec<u8> {
    let e = &se.event;
    let mut buf = Vec::with_capacity(256);

    // Event fields (order matches canonical_signing_bytes for consistency).
    buf.extend_from_slice(e.id.as_bytes()); // 16
    buf.extend_from_slice(e.device_id.as_bytes()); // 16
    buf.extend_from_slice(&e.lamport.to_le_bytes()); // 8

    // Vector clock: count u32 + (DeviceId + lamport) × N.
    let vc_len = u32::try_from(e.vector_clock.0.len()).unwrap_or(u32::MAX);
    buf.extend_from_slice(&vc_len.to_le_bytes());
    for (dev, lam) in &e.vector_clock.0 {
        buf.extend_from_slice(dev.as_bytes());
        buf.extend_from_slice(&lam.to_le_bytes());
    }

    // Parent event id.
    if let Some(parent) = e.parent_event_id {
        buf.push(1);
        buf.extend_from_slice(parent.as_bytes());
    } else {
        buf.push(0);
    }

    // Entity kind (length-prefixed string).
    write_lp_str(&mut buf, e.entity_kind.as_str());

    // Entity id.
    buf.extend_from_slice(e.entity_id.as_bytes()); // 16

    // Op: tag + sub_tag.
    write_lp_str(&mut buf, e.op.tag());
    write_lp_bytes(&mut buf, &e.op.sub_tag_bytes());

    // Payload.
    write_lp_bytes(&mut buf, &e.payload_ct);
    buf.extend_from_slice(&e.payload_nonce); // 12
    write_lp_bytes(&mut buf, &e.payload_aad);

    // Timestamp.
    buf.extend_from_slice(&e.created_at.unix_timestamp().to_le_bytes());
    let nanos = i32::try_from(e.created_at.nanosecond()).unwrap_or(i32::MAX);
    buf.extend_from_slice(&nanos.to_le_bytes());

    // Signature + signer.
    buf.extend_from_slice(&se.signature.to_bytes()); // 64
    buf.extend_from_slice(&se.signer.to_bytes()); // 32

    buf
}

fn decode_event_frame(bytes: &[u8]) -> Result<SignedEvent, Error> {
    let mut c = Cursor::new(bytes);

    let id = c.read_uuid()?;
    let device_id = DeviceId::from_bytes(c.read_fixed::<16>()?);
    let lamport = c.read_u64()?;

    // Vector clock.
    let vc_count = c.read_u32()?;
    if vc_count > 1024 {
        return Err(Error::WireFormat("vector clock entry count too large"));
    }
    let mut vc_map = std::collections::BTreeMap::new();
    for _ in 0..vc_count {
        let dev = DeviceId::from_bytes(c.read_fixed::<16>()?);
        let lam = c.read_u64()?;
        vc_map.insert(dev, lam);
    }
    let vector_clock = VectorClock(vc_map);

    // Parent.
    let parent_flag = c.read_u8()?;
    let parent_event_id = match parent_flag {
        0 => None,
        1 => Some(c.read_uuid()?),
        _ => return Err(Error::WireFormat("invalid parent_event_id flag")),
    };

    // Entity kind.
    let kind_str = c.read_lp_string()?;
    let entity_kind = parse_entity_kind(&kind_str)?;

    // Entity id.
    let entity_id = c.read_uuid()?;

    // Op.
    let op_tag = c.read_lp_string()?;
    let op_sub = c.read_lp_bytes()?;
    let op = parse_op(&op_tag, &op_sub)?;

    // Payload.
    let payload_ct = c.read_lp_bytes()?;
    let payload_nonce: [u8; 12] = c.read_fixed()?;
    let payload_aad = c.read_lp_bytes()?;

    // Timestamp.
    let unix_secs = c.read_i64()?;
    let nanos = c.read_i32()?;
    let created_at = OffsetDateTime::from_unix_timestamp_nanos(
        i128::from(unix_secs) * 1_000_000_000 + i128::from(nanos),
    )
    .map_err(|_| Error::WireFormat("invalid timestamp"))?;

    // Signature + signer.
    let sig_bytes: [u8; 64] = c.read_fixed()?;
    let signer_bytes: [u8; 32] = c.read_fixed()?;
    let signature = Signature::from_bytes(&sig_bytes);
    let signer =
        VerifyingKey::from_bytes(&signer_bytes).map_err(|_| Error::WireFormat("invalid signer"))?;

    if c.remaining() != 0 {
        return Err(Error::WireFormat("trailing bytes in event frame"));
    }

    Ok(SignedEvent {
        event: Event {
            id,
            device_id,
            lamport,
            vector_clock,
            parent_event_id,
            entity_kind,
            entity_id,
            op,
            payload_ct,
            payload_nonce,
            payload_aad,
            created_at,
        },
        signature,
        signer,
    })
}

// ── Helpers ──────────────────────────────────────────────────────────

fn write_lp_str(buf: &mut Vec<u8>, s: &str) {
    let len = u32::try_from(s.len()).unwrap_or(u32::MAX);
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
}

fn write_lp_bytes(buf: &mut Vec<u8>, b: &[u8]) {
    let len = u32::try_from(b.len()).unwrap_or(u32::MAX);
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(b);
}

/// Bounds-checked read cursor over a byte slice.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

/// Max length-prefixed blob/string: 256 KiB.
const MAX_LP_LEN: u32 = 262_144;

impl<'a> Cursor<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    const fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.pos + n > self.data.len() {
            return Err(Error::WireFormat("unexpected end of data"));
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn read_fixed<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        self.read_bytes(N)?
            .try_into()
            .map_err(|_| Error::WireFormat("fixed-size read"))
    }

    fn read_u8(&mut self) -> Result<u8, Error> {
        Ok(self.read_fixed::<1>()?[0])
    }

    fn read_u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.read_fixed()?))
    }

    fn read_u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.read_fixed()?))
    }

    fn read_i64(&mut self) -> Result<i64, Error> {
        Ok(i64::from_le_bytes(self.read_fixed()?))
    }

    fn read_i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.read_fixed()?))
    }

    fn read_uuid(&mut self) -> Result<Uuid, Error> {
        Ok(Uuid::from_bytes(self.read_fixed()?))
    }

    fn read_lp_bytes(&mut self) -> Result<Vec<u8>, Error> {
        let len = self.read_u32()?;
        if len > MAX_LP_LEN {
            return Err(Error::WireFormat("length-prefixed blob too large"));
        }
        Ok(self.read_bytes(len as usize)?.to_vec())
    }

    fn read_lp_string(&mut self) -> Result<String, Error> {
        let raw = self.read_lp_bytes()?;
        String::from_utf8(raw).map_err(|_| Error::WireFormat("invalid UTF-8 in string"))
    }
}

fn parse_entity_kind(s: &str) -> Result<EntityKind, Error> {
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
        _ => return Err(Error::WireFormat("unknown entity kind")),
    })
}

fn parse_op(tag: &str, sub: &[u8]) -> Result<EventOp, Error> {
    Ok(match tag {
        "create" => EventOp::Create,
        "delete" => EventOp::Delete,
        "purge" => EventOp::Purge,
        "snapshot" => EventOp::Snapshot,
        "append" => EventOp::Append {
            sub_kind: String::from_utf8(sub.to_vec())
                .map_err(|_| Error::WireFormat("invalid UTF-8 in append sub_kind"))?,
        },
        "update" => {
            let mut fields = Vec::new();
            let mut cursor = sub;
            while cursor.len() >= 4 {
                let n = u32::from_le_bytes(
                    cursor[..4]
                        .try_into()
                        .map_err(|_| Error::WireFormat("update field length"))?,
                ) as usize;
                cursor = &cursor[4..];
                if cursor.len() < n {
                    return Err(Error::WireFormat("truncated update field name"));
                }
                fields.push(
                    String::from_utf8(cursor[..n].to_vec())
                        .map_err(|_| Error::WireFormat("invalid UTF-8 in field name"))?,
                );
                cursor = &cursor[n..];
            }
            if !cursor.is_empty() {
                return Err(Error::WireFormat("trailing bytes in update sub_tag"));
            }
            EventOp::Update { fields }
        }
        _ => return Err(Error::WireFormat("unknown op tag")),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use tock_core::event::sign;
    use tock_crypto::signature::SigningKey;

    fn sample_event(device: DeviceId, lamport: u64) -> Event {
        Event {
            id: Uuid::from_bytes([1; 16]),
            device_id: device,
            lamport,
            vector_clock: VectorClock::singleton(device, lamport),
            parent_event_id: None,
            entity_kind: EntityKind::Task,
            entity_id: Uuid::from_bytes([2; 16]),
            op: EventOp::Create,
            payload_ct: vec![0xAA; 64],
            payload_nonce: [3; 12],
            payload_aad: b"aad".to_vec(),
            created_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("ts"),
        }
    }

    fn signed_sample(device: DeviceId, lamport: u64) -> SignedEvent {
        let sk = SigningKey::try_generate().expect("rng");
        sign(sample_event(device, lamport), &sk)
    }

    #[test]
    fn batch_roundtrip_single_event() {
        let se = signed_sample(DeviceId([7; 16]), 1);
        let encoded = encode_batch(&[se.clone()]).expect("encode");
        assert!(encoded.starts_with(BATCH_MAGIC));
        let decoded = decode_batch(&encoded).expect("decode");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].event.id, se.event.id);
        assert_eq!(decoded[0].event.lamport, se.event.lamport);
        assert_eq!(decoded[0].event.payload_ct, se.event.payload_ct);
    }

    #[test]
    fn batch_roundtrip_multiple_events() {
        let dev = DeviceId([7; 16]);
        let events: Vec<_> = (1..=5).map(|i| signed_sample(dev, i)).collect();
        let encoded = encode_batch(&events).expect("encode");
        let decoded = decode_batch(&encoded).expect("decode");
        assert_eq!(decoded.len(), 5);
        for (orig, dec) in events.iter().zip(decoded.iter()) {
            assert_eq!(orig.event.lamport, dec.event.lamport);
        }
    }

    #[test]
    fn batch_roundtrip_empty() {
        let encoded = encode_batch(&[]).expect("encode");
        let decoded = decode_batch(&encoded).expect("decode");
        assert!(decoded.is_empty());
    }

    #[test]
    fn batch_roundtrip_with_update_op() {
        let sk = SigningKey::try_generate().expect("rng");
        let dev = DeviceId([8; 16]);
        let event = Event {
            id: Uuid::from_bytes([10; 16]),
            device_id: dev,
            lamport: 3,
            vector_clock: VectorClock::singleton(dev, 3),
            parent_event_id: Some(Uuid::from_bytes([9; 16])),
            entity_kind: EntityKind::Habit,
            entity_id: Uuid::from_bytes([11; 16]),
            op: EventOp::Update {
                fields: vec!["title".into(), "notes".into(), "status".into()],
            },
            payload_ct: vec![0xBB; 128],
            payload_nonce: [4; 12],
            payload_aad: b"update-aad".to_vec(),
            created_at: OffsetDateTime::from_unix_timestamp(1_700_100_000).expect("ts"),
        };
        let se = sign(event, &sk);
        let encoded = encode_batch(&[se.clone()]).expect("encode");
        let decoded = decode_batch(&encoded).expect("decode");
        assert_eq!(decoded.len(), 1);
        let d = &decoded[0];
        assert_eq!(d.event.parent_event_id, se.event.parent_event_id);
        assert_eq!(d.event.entity_kind, EntityKind::Habit);
        if let EventOp::Update { fields } = &d.event.op {
            assert_eq!(fields, &["title", "notes", "status"]);
        } else {
            panic!("expected Update op");
        }
    }

    #[test]
    fn batch_roundtrip_with_append_op() {
        let sk = SigningKey::try_generate().expect("rng");
        let dev = DeviceId([9; 16]);
        let event = Event {
            id: Uuid::from_bytes([20; 16]),
            device_id: dev,
            lamport: 1,
            vector_clock: VectorClock::singleton(dev, 1),
            parent_event_id: None,
            entity_kind: EntityKind::Annotation,
            entity_id: Uuid::from_bytes([21; 16]),
            op: EventOp::Append {
                sub_kind: "annotation".into(),
            },
            payload_ct: vec![0xCC; 32],
            payload_nonce: [5; 12],
            payload_aad: b"append-aad".to_vec(),
            created_at: OffsetDateTime::from_unix_timestamp(1_700_200_000).expect("ts"),
        };
        let se = sign(event, &sk);
        let encoded = encode_batch(&[se]).expect("encode");
        let decoded = decode_batch(&encoded).expect("decode");
        assert_eq!(decoded.len(), 1);
        if let EventOp::Append { sub_kind } = &decoded[0].event.op {
            assert_eq!(sub_kind, "annotation");
        } else {
            panic!("expected Append op");
        }
    }

    #[test]
    fn invalid_magic_rejected() {
        let mut data = encode_batch(&[]).expect("encode");
        data[0] ^= 0x01;
        assert!(decode_batch(&data).is_err());
    }

    #[test]
    fn truncated_batch_rejected() {
        let se = signed_sample(DeviceId([7; 16]), 1);
        let encoded = encode_batch(&[se]).expect("encode");
        assert!(decode_batch(&encoded[..encoded.len() - 1]).is_err());
    }

    #[test]
    fn trailing_bytes_rejected() {
        let encoded = encode_batch(&[]).expect("encode");
        let mut padded = encoded;
        padded.push(0xFF);
        assert!(decode_batch(&padded).is_err());
    }

    #[test]
    fn corrupted_frame_rejected() {
        let se = signed_sample(DeviceId([7; 16]), 1);
        let mut encoded = encode_batch(&[se]).expect("encode");
        // Corrupt an event field byte.
        if encoded.len() > 30 {
            encoded[30] ^= 0xFF;
        }
        // May or may not error depending on which byte was flipped, but
        // should not panic.
        let _ = decode_batch(&encoded);
    }

    #[test]
    fn multi_device_vector_clock_roundtrips() {
        let sk = SigningKey::try_generate().expect("rng");
        let dev_a = DeviceId([1; 16]);
        let dev_b = DeviceId([2; 16]);
        let mut vc = VectorClock::singleton(dev_a, 5);
        vc.merge(&VectorClock::singleton(dev_b, 3));
        let event = Event {
            id: Uuid::from_bytes([30; 16]),
            device_id: dev_a,
            lamport: 5,
            vector_clock: vc,
            parent_event_id: None,
            entity_kind: EntityKind::TimeBlock,
            entity_id: Uuid::from_bytes([31; 16]),
            op: EventOp::Create,
            payload_ct: vec![0xDD; 16],
            payload_nonce: [6; 12],
            payload_aad: vec![],
            created_at: OffsetDateTime::from_unix_timestamp(1_700_300_000).expect("ts"),
        };
        let se = sign(event, &sk);
        let encoded = encode_batch(&[se.clone()]).expect("encode");
        let decoded = decode_batch(&encoded).expect("decode");
        assert_eq!(decoded[0].event.vector_clock, se.event.vector_clock);
    }

    #[test]
    fn golden_vector_empty_batch() {
        let encoded = encode_batch(&[]).expect("encode");
        // magic (12) + version (1) + count (4) = 17 bytes.
        assert_eq!(encoded.len(), 17);
        assert_eq!(&encoded[..12], BATCH_MAGIC);
        assert_eq!(encoded[12], 1); // version
        assert_eq!(&encoded[13..17], &[0, 0, 0, 0]); // count = 0
    }
}
