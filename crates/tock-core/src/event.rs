//! Event log primitives — pure types, canonical signing payload, and
//! `sign`/`verify` helpers.
//!
//! Architecture §6.1 defines the event schema. Issue #5 adds the
//! mandate that events are signed with Ed25519 so any tampering with
//! the persisted log surfaces on read.
//!
//! ## Signature scope
//!
//! Every semantic field of the event is covered by the signature:
//! `id`, `device_id`, `lamport`, `vector_clock`, `parent_event_id`,
//! `entity_kind`, `entity_id`, `op`, the encrypted payload triple
//! (`payload_ct`, `payload_nonce`, `payload_aad`), `created_at`, and
//! the signer's verifying-key bytes. The canonical serialization is
//! domain-tagged (`b"tock-event-sig-v1"`), length-prefixed for every
//! byte field, and sorts `vector_clock` entries by `DeviceId` so the
//! encoding is deterministic.
//!
//! ## Device-binding
//!
//! `device_id` is a random 16-byte identifier; the corresponding
//! Ed25519 verifying key lives in a device registry that the storage
//! layer maintains. [`verify`] takes a closure that resolves a
//! `device_id` to its registered verifying key and rejects events
//! whose claimed device is unknown or signed by a different key.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tock_crypto::signature::{Signature, SigningKey, VerifyingKey};
use uuid::Uuid;

use crate::Error;

/// 16-byte device identifier (random, generated at vault init).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub [u8; 16]);

impl DeviceId {
    /// Construct from raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// View the raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Vector clock — mapping of device → highest seen Lamport timestamp.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock(pub BTreeMap<DeviceId, u64>);

impl VectorClock {
    /// Empty vector clock.
    #[must_use]
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Single-device vector clock with the given Lamport value.
    #[must_use]
    pub fn singleton(device: DeviceId, lamport: u64) -> Self {
        let mut m = BTreeMap::new();
        m.insert(device, lamport);
        Self(m)
    }

    /// Per-device max merge.
    pub fn merge(&mut self, other: &Self) {
        for (k, v) in &other.0 {
            let entry = self.0.entry(*k).or_insert(0);
            if *v > *entry {
                *entry = *v;
            }
        }
    }

    /// True iff `self` ≤ `other` and `self` ≠ `other` (strict happens-before).
    #[must_use]
    pub fn happens_before(&self, other: &Self) -> bool {
        let mut any_less = false;
        for (k, sv) in &self.0 {
            let ov = other.0.get(k).copied().unwrap_or(0);
            if *sv > ov {
                return false;
            }
            if *sv < ov {
                any_less = true;
            }
        }
        for (k, ov) in &other.0 {
            if !self.0.contains_key(k) && *ov > 0 {
                any_less = true;
            }
        }
        any_less
    }

    /// True iff neither vector dominates the other and they are not equal.
    #[must_use]
    #[allow(clippy::suspicious_operation_groupings)]
    pub fn concurrent_with(&self, other: &Self) -> bool {
        !self.happens_before(other) && !other.happens_before(self) && self != other
    }
}

/// Entity types tracked in the event log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityKind {
    /// A task.
    Task,
    /// A project.
    Project,
    /// A heading inside a project.
    Heading,
    /// An area.
    Area,
    /// A habit.
    Habit,
    /// A logged habit entry.
    HabitEntry,
    /// A time-tracking block.
    TimeBlock,
    /// A focus (Pomodoro) session.
    FocusSession,
    /// A free-form annotation attached to some entity.
    Annotation,
    /// A device registration record.
    Device,
}

impl EntityKind {
    /// Canonical wire-format string for the entity kind.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Project => "project",
            Self::Heading => "heading",
            Self::Area => "area",
            Self::Habit => "habit",
            Self::HabitEntry => "habit_entry",
            Self::TimeBlock => "time_block",
            Self::FocusSession => "focus_session",
            Self::Annotation => "annotation",
            Self::Device => "device",
        }
    }
}

/// Operations producible by the event log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventOp {
    /// New entity created. Payload carries the full initial state.
    Create,
    /// Entity updated. Payload carries the changed fields and their
    /// new values, plus the field names for sync field-level merge.
    Update {
        /// Names of fields updated.
        fields: Vec<String>,
    },
    /// Entity soft-deleted (sets `deleted_at`).
    Delete,
    /// Entity hard-deleted (tombstoned).
    Purge,
    /// Append-only addition to a sub-collection.
    Append {
        /// Sub-kind identifier (e.g. `annotation`, `habit_entry`).
        sub_kind: String,
    },
    /// Snapshot event produced by compaction.
    Snapshot,
}

impl EventOp {
    /// Canonical wire-format string for the variant tag (no payload).
    #[must_use]
    pub const fn tag(&self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update { .. } => "update",
            Self::Delete => "delete",
            Self::Purge => "purge",
            Self::Append { .. } => "append",
            Self::Snapshot => "snapshot",
        }
    }

    /// Sub-tag bytes for the variant (the field list for `Update`, the
    /// sub-kind for `Append`, empty for the rest). Included in the
    /// canonical signing payload so it's covered by the signature.
    #[must_use]
    pub fn sub_tag_bytes(&self) -> Vec<u8> {
        match self {
            Self::Update { fields } => {
                let mut out = Vec::new();
                for f in fields {
                    out.extend_from_slice(
                        &u32::try_from(f.len()).unwrap_or(u32::MAX).to_le_bytes(),
                    );
                    out.extend_from_slice(f.as_bytes());
                }
                out
            }
            Self::Append { sub_kind } => sub_kind.as_bytes().to_vec(),
            _ => Vec::new(),
        }
    }
}

/// An event in the append-only log.
///
/// The payload is encrypted at rest with the per-entity item key (see
/// `tock_core::vault::KeyHierarchy::derive_item_key`); this struct
/// holds the ciphertext, not the plaintext.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// `UUIDv7` — time-ordered, globally unique.
    pub id: Uuid,
    /// Device that produced this event.
    pub device_id: DeviceId,
    /// Monotonically increasing Lamport timestamp for `device_id`.
    pub lamport: u64,
    /// Vector clock as of event production.
    pub vector_clock: VectorClock,
    /// Previous local event id (chain integrity). `None` for the very
    /// first event a device produces.
    pub parent_event_id: Option<Uuid>,
    /// Entity kind addressed by this event.
    pub entity_kind: EntityKind,
    /// Entity id addressed by this event (`UUIDv7`).
    pub entity_id: Uuid,
    /// Operation applied.
    pub op: EventOp,
    /// AEAD ciphertext of the (size-padded) payload.
    pub payload_ct: Vec<u8>,
    /// AEAD nonce (12 bytes).
    pub payload_nonce: [u8; 12],
    /// AEAD associated data (binds the ciphertext to the event identity).
    pub payload_aad: Vec<u8>,
    /// Wall-clock time of event production.
    pub created_at: OffsetDateTime,
}

/// A signed event = event + Ed25519 signature + signer's verifying key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedEvent {
    /// The event itself.
    pub event: Event,
    /// Ed25519 signature over the canonical signing payload (see
    /// [`canonical_signing_bytes`]).
    pub signature: Signature,
    /// Verifying key the signer used. The verifier consults the device
    /// registry to confirm this key matches the registered key for
    /// `event.device_id`.
    pub signer: VerifyingKey,
}

/// Build the canonical, domain-tagged byte sequence that Ed25519 signs.
///
/// Layout (all integers little-endian, byte fields length-prefixed with
/// a `u32` length):
///
/// ```text
/// "tock-event-sig-v1"
/// || event.id                       (16 bytes)
/// || event.device_id                (16 bytes)
/// || event.lamport                  (u64)
/// || vector_clock_len u32
///    || (DeviceId[16] || lamport u64) × N   (sorted ascending by DeviceId)
/// || parent_present u8 || optional event.parent_event_id (16 bytes)
/// || entity_kind: u32 len || bytes
/// || entity_id                      (16 bytes)
/// || op.tag(): u32 len || bytes
/// || op.sub_tag_bytes(): u32 len || bytes
/// || payload_ct: u32 len || bytes
/// || payload_nonce                  (12 bytes)
/// || payload_aad: u32 len || bytes
/// || created_at_unix_seconds i64 || created_at_nanos i32
/// || signer_verifying_key           (32 bytes)
/// ```
#[must_use]
pub fn canonical_signing_bytes(event: &Event, signer: &VerifyingKey) -> Vec<u8> {
    const DOMAIN: &[u8] = b"tock-event-sig-v1";
    let mut out = Vec::with_capacity(DOMAIN.len() + 256);
    out.extend_from_slice(DOMAIN);
    out.extend_from_slice(event.id.as_bytes());
    out.extend_from_slice(event.device_id.as_bytes());
    out.extend_from_slice(&event.lamport.to_le_bytes());

    let vc_len = u32::try_from(event.vector_clock.0.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&vc_len.to_le_bytes());
    for (dev, lam) in &event.vector_clock.0 {
        out.extend_from_slice(dev.as_bytes());
        out.extend_from_slice(&lam.to_le_bytes());
    }

    if let Some(parent) = event.parent_event_id {
        out.push(1);
        out.extend_from_slice(parent.as_bytes());
    } else {
        out.push(0);
    }

    let kind = event.entity_kind.as_str().as_bytes();
    out.extend_from_slice(&u32::try_from(kind.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(kind);

    out.extend_from_slice(event.entity_id.as_bytes());

    let op_tag = event.op.tag().as_bytes();
    out.extend_from_slice(
        &u32::try_from(op_tag.len())
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    out.extend_from_slice(op_tag);

    let sub = event.op.sub_tag_bytes();
    out.extend_from_slice(&u32::try_from(sub.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&sub);

    out.extend_from_slice(
        &u32::try_from(event.payload_ct.len())
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    out.extend_from_slice(&event.payload_ct);
    out.extend_from_slice(&event.payload_nonce);
    out.extend_from_slice(
        &u32::try_from(event.payload_aad.len())
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    out.extend_from_slice(&event.payload_aad);

    out.extend_from_slice(&event.created_at.unix_timestamp().to_le_bytes());
    let nanos = i32::try_from(event.created_at.nanosecond()).unwrap_or(i32::MAX);
    out.extend_from_slice(&nanos.to_le_bytes());

    out.extend_from_slice(&signer.to_bytes());
    out
}

/// Sign `event` with `signing_key`.
///
/// The verifying key derived from `signing_key` is included in the
/// returned [`SignedEvent`] (so a receiver who has not yet seen this
/// device's registration event can still locate the key, then look it
/// up in the registry to verify the binding).
#[must_use]
pub fn sign(event: Event, signing_key: &SigningKey) -> SignedEvent {
    let signer = signing_key.verifying_key();
    let bytes = canonical_signing_bytes(&event, &signer);
    let signature = signing_key.sign(&bytes);
    SignedEvent {
        event,
        signature,
        signer,
    }
}

/// Verify a signed event.
///
/// `lookup_device_key` maps a `DeviceId` to the verifying key
/// registered for that device (typically backed by a `devices` table
/// in the storage layer). Verification fails if:
///
/// - the device is unknown,
/// - the device's registered key differs from `signed.signer`,
/// - the Ed25519 signature does not validate.
///
/// # Errors
/// Returns [`Error::InvalidSignature`] for any of the failure modes
/// above. The cause is not distinguished.
pub fn verify<F>(signed: &SignedEvent, lookup_device_key: F) -> Result<(), Error>
where
    F: FnOnce(&DeviceId) -> Option<VerifyingKey>,
{
    let Some(registered) = lookup_device_key(&signed.event.device_id) else {
        return Err(Error::InvalidSignature);
    };
    if registered != signed.signer {
        return Err(Error::InvalidSignature);
    }
    let bytes = canonical_signing_bytes(&signed.event, &signed.signer);
    signed
        .signer
        .verify(&bytes, &signed.signature)
        .map_err(|_| Error::InvalidSignature)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    fn sample_event(device: DeviceId) -> Event {
        Event {
            id: Uuid::from_bytes([1; 16]),
            device_id: device,
            lamport: 1,
            vector_clock: VectorClock::singleton(device, 1),
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

    #[test]
    fn sign_then_verify_with_correct_registry() {
        let sk = SigningKey::try_generate().expect("rng");
        let dev = DeviceId([7; 16]);
        let signed = sign(sample_event(dev), &sk);
        let vk = sk.verifying_key();
        verify(&signed, |id| (*id == dev).then_some(vk)).expect("verify");
    }

    #[test]
    fn unknown_device_rejected() {
        let sk = SigningKey::try_generate().expect("rng");
        let signed = sign(sample_event(DeviceId([7; 16])), &sk);
        assert!(matches!(
            verify(&signed, |_| None),
            Err(Error::InvalidSignature)
        ));
    }

    #[test]
    fn mismatched_registry_key_rejected() {
        let sk = SigningKey::try_generate().expect("rng");
        let other = SigningKey::try_generate().expect("rng");
        let dev = DeviceId([7; 16]);
        let signed = sign(sample_event(dev), &sk);
        // Registry returns `other`'s key for the device — must be rejected.
        let other_vk = other.verifying_key();
        assert!(matches!(
            verify(&signed, |id| (*id == dev).then_some(other_vk)),
            Err(Error::InvalidSignature)
        ));
    }

    #[test]
    fn tampered_payload_fails_verify() {
        let sk = SigningKey::try_generate().expect("rng");
        let dev = DeviceId([7; 16]);
        let mut signed = sign(sample_event(dev), &sk);
        signed.event.payload_ct[0] ^= 0x01;
        let vk = sk.verifying_key();
        assert!(matches!(
            verify(&signed, |id| (*id == dev).then_some(vk)),
            Err(Error::InvalidSignature)
        ));
    }

    #[test]
    fn tampered_vector_clock_fails_verify() {
        let sk = SigningKey::try_generate().expect("rng");
        let dev = DeviceId([7; 16]);
        let mut signed = sign(sample_event(dev), &sk);
        signed.event.vector_clock.0.insert(DeviceId([99; 16]), 42);
        let vk = sk.verifying_key();
        assert!(matches!(
            verify(&signed, |id| (*id == dev).then_some(vk)),
            Err(Error::InvalidSignature)
        ));
    }

    #[test]
    fn canonical_bytes_change_for_every_field() {
        let sk = SigningKey::try_generate().expect("rng");
        let vk = sk.verifying_key();
        let dev = DeviceId([7; 16]);
        let base = sample_event(dev);
        let base_bytes = canonical_signing_bytes(&base, &vk);

        let mut e = base.clone();
        e.lamport += 1;
        assert_ne!(canonical_signing_bytes(&e, &vk), base_bytes);

        let mut e = base.clone();
        e.payload_nonce[0] ^= 1;
        assert_ne!(canonical_signing_bytes(&e, &vk), base_bytes);

        let mut e = base;
        e.op = EventOp::Update {
            fields: vec!["title".into()],
        };
        assert_ne!(canonical_signing_bytes(&e, &vk), base_bytes);
    }

    #[test]
    fn vector_clock_happens_before_and_merge() {
        let a = DeviceId([1; 16]);
        let b = DeviceId([2; 16]);
        let v1 = VectorClock::singleton(a, 1);
        let mut v2 = v1.clone();
        v2.0.insert(b, 1);
        assert!(v1.happens_before(&v2));
        assert!(!v2.happens_before(&v1));
        let mut merged = v1;
        merged.merge(&VectorClock::singleton(b, 5));
        assert_eq!(merged.0.get(&b).copied(), Some(5));
    }
}
