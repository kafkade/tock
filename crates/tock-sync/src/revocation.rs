//! Device revocation — append-only status changes for the device
//! registry.
//!
//! When a device is revoked, it remains in the registry so historical
//! event signatures can still be verified, but new events from it are
//! rejected. Revocation is broadcast as a sync event so all devices
//! converge on the same revoked set.
//!
//! ## Authorization
//!
//! Only an active device can revoke another device. A device cannot
//! revoke itself (self-revocation would create an unverifiable event).

use time::OffsetDateTime;
use tock_core::event::DeviceId;
use uuid::Uuid;

use crate::Error;

/// Status of a device in the registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceStatus {
    /// Device is active and can produce/consume events.
    Active,
    /// Device has been revoked.
    Revoked {
        /// When the revocation was recorded.
        revoked_at: OffsetDateTime,
        /// Which device performed the revocation.
        revoked_by: DeviceId,
        /// Human-readable reason (e.g. "lost", "compromised").
        reason: String,
    },
}

impl DeviceStatus {
    /// True if the device is active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// True if the device has been revoked.
    #[must_use]
    pub const fn is_revoked(&self) -> bool {
        matches!(self, Self::Revoked { .. })
    }
}

/// A record of a device revocation for the conflict log / audit trail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevocationRecord {
    /// The device being revoked.
    pub target_device: DeviceId,
    /// When the revocation was issued.
    pub revoked_at: OffsetDateTime,
    /// Which device performed the revocation.
    pub revoked_by: DeviceId,
    /// Human-readable reason.
    pub reason: String,
    /// The event id that carries this revocation.
    pub event_id: Uuid,
}

/// A device entry in the registry with its current status.
#[derive(Clone, Debug)]
pub struct DeviceRegistryEntry {
    /// Device identifier.
    pub device_id: DeviceId,
    /// Current status.
    pub status: DeviceStatus,
    /// Ed25519 verifying key bytes (kept for historical verification).
    pub verifying_key: [u8; 32],
    /// Optional human label.
    pub label: Option<String>,
    /// When the device was first registered.
    pub registered_at: OffsetDateTime,
}

/// Validate that a revocation request is authorized.
///
/// Rules:
/// - The revoker must be in the registry and active.
/// - The target must be in the registry and active (can't revoke
///   an already-revoked or unknown device).
/// - A device cannot revoke itself.
///
/// # Errors
/// Returns [`Error::Core`] with a descriptive message on validation
/// failure.
pub fn validate_revocation(
    revoker: DeviceId,
    target: DeviceId,
    registry: &[DeviceRegistryEntry],
) -> Result<(), Error> {
    if revoker == target {
        return Err(Error::WireFormat("device cannot revoke itself"));
    }

    let revoker_entry = registry
        .iter()
        .find(|e| e.device_id == revoker)
        .ok_or(Error::WireFormat("revoker device not in registry"))?;

    if !revoker_entry.status.is_active() {
        return Err(Error::WireFormat("revoker device is not active"));
    }

    let target_entry = registry
        .iter()
        .find(|e| e.device_id == target)
        .ok_or(Error::WireFormat("target device not in registry"))?;

    if !target_entry.status.is_active() {
        return Err(Error::WireFormat("target device is already revoked"));
    }

    Ok(())
}

/// Check whether a device is revoked.
#[must_use]
pub fn is_device_revoked(device: DeviceId, revocations: &[RevocationRecord]) -> bool {
    revocations.iter().any(|r| r.target_device == device)
}

/// Apply a revocation to a device registry entry.
///
/// Returns the updated entry. The original verifying key is preserved
/// for historical signature verification.
#[must_use]
pub fn apply_revocation(
    entry: &DeviceRegistryEntry,
    revoked_by: DeviceId,
    reason: &str,
) -> DeviceRegistryEntry {
    DeviceRegistryEntry {
        device_id: entry.device_id,
        status: DeviceStatus::Revoked {
            revoked_at: OffsetDateTime::now_utc(),
            revoked_by,
            reason: reason.to_string(),
        },
        verifying_key: entry.verifying_key,
        label: entry.label.clone(),
        registered_at: entry.registered_at,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    fn make_entry(id: [u8; 16], active: bool) -> DeviceRegistryEntry {
        DeviceRegistryEntry {
            device_id: DeviceId(id),
            status: if active {
                DeviceStatus::Active
            } else {
                DeviceStatus::Revoked {
                    revoked_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("ts"),
                    revoked_by: DeviceId([99; 16]),
                    reason: "test".into(),
                }
            },
            verifying_key: [0xAA; 32],
            label: Some("test device".into()),
            registered_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("ts"),
        }
    }

    #[test]
    fn valid_revocation_succeeds() {
        let revoker = make_entry([1; 16], true);
        let target = make_entry([2; 16], true);
        let registry = vec![revoker, target];
        assert!(validate_revocation(DeviceId([1; 16]), DeviceId([2; 16]), &registry).is_ok());
    }

    #[test]
    fn self_revocation_rejected() {
        let dev = make_entry([1; 16], true);
        let registry = vec![dev];
        assert!(validate_revocation(DeviceId([1; 16]), DeviceId([1; 16]), &registry).is_err());
    }

    #[test]
    fn revoked_revoker_rejected() {
        let revoker = make_entry([1; 16], false);
        let target = make_entry([2; 16], true);
        let registry = vec![revoker, target];
        assert!(validate_revocation(DeviceId([1; 16]), DeviceId([2; 16]), &registry).is_err());
    }

    #[test]
    fn already_revoked_target_rejected() {
        let revoker = make_entry([1; 16], true);
        let target = make_entry([2; 16], false);
        let registry = vec![revoker, target];
        assert!(validate_revocation(DeviceId([1; 16]), DeviceId([2; 16]), &registry).is_err());
    }

    #[test]
    fn unknown_device_rejected() {
        let revoker = make_entry([1; 16], true);
        let registry = vec![revoker];
        assert!(validate_revocation(DeviceId([1; 16]), DeviceId([99; 16]), &registry).is_err());
    }

    #[test]
    fn is_device_revoked_check() {
        let record = RevocationRecord {
            target_device: DeviceId([2; 16]),
            revoked_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("ts"),
            revoked_by: DeviceId([1; 16]),
            reason: "compromised".into(),
            event_id: Uuid::from_bytes([50; 16]),
        };
        assert!(is_device_revoked(DeviceId([2; 16]), &[record.clone()]));
        assert!(!is_device_revoked(DeviceId([3; 16]), &[record]));
    }

    #[test]
    fn apply_revocation_preserves_key() {
        let entry = make_entry([2; 16], true);
        let updated = apply_revocation(&entry, DeviceId([1; 16]), "lost");
        assert!(updated.status.is_revoked());
        assert_eq!(updated.verifying_key, entry.verifying_key);
        assert_eq!(updated.device_id, entry.device_id);
    }
}
