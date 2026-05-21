//! # tock-storage
//!
//! `SQLite` storage adapter for tock. Implements the vault format and
//! the append-only event log on top of `rusqlite` (with `bundled`
//! `SQLite`). Sensitive data is AEAD-encrypted at the application
//! layer via `tock-crypto`; the on-disk `vault_meta` table records a
//! `storage_layout` marker so a future `SQLCipher` integration can
//! detect and upgrade this Phase 0 format.
//!
//! See `docs/architecture.md` §3 and §5, ADR-002, and ADR-004 for the
//! design.

pub mod error;
pub mod event_log;
pub mod migrations;
pub mod vault;

pub use error::Error;
pub use event_log::EventLog;
pub use vault::{LocalDevice, OpenVault, VaultStatus, init, open, status};

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::missing_const_for_fn,
        clippy::panic
    )]

    use super::*;
    use tempfile::tempdir;
    use tock_core::event::{EntityKind, EventOp, VectorClock};
    use uuid::Uuid;

    fn write_then_read_one() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("v.tockvault");
        let v = init(&path, b"pw").expect("init");
        let device_id = v.local_device().device_id;
        let entity = Uuid::now_v7();
        let log = EventLog::new(&v);
        let signed = log
            .append(
                EntityKind::Task,
                entity,
                EventOp::Create,
                b"hello-tock-payload",
                VectorClock::singleton(tock_core::event::DeviceId::from_bytes(device_id), 1),
                None,
            )
            .expect("append");
        assert_eq!(signed.event.entity_id, entity);
        let read = log.read_all().expect("read");
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].1.as_slice(), b"hello-tock-payload");
    }

    #[test]
    fn vault_init_status_lock_reopen_roundtrip() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("v.tockvault");
        let v = init(&path, b"hunter2").expect("init");
        let vault_id = v.header().vault_id;
        let stat_before = status(&path).expect("status");
        assert_eq!(stat_before.vault_id, vault_id);
        v.lock();

        let v2 = open(&path, b"hunter2").expect("open");
        assert_eq!(v2.header().vault_id, vault_id);
    }

    #[test]
    fn wrong_password_is_invalid_credentials() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("v.tockvault");
        init(&path, b"hunter2").expect("init").lock();
        match open(&path, b"wrong") {
            Err(Error::InvalidVaultOrCredentials) => {}
            other => panic!("expected InvalidVaultOrCredentials, got {other:?}"),
        }
    }

    #[test]
    fn missing_file_is_not_found() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("nope.tockvault");
        assert!(matches!(open(&path, b"pw"), Err(Error::NotFound)));
        assert!(matches!(status(&path), Err(Error::NotFound)));
    }

    #[test]
    fn append_and_read_roundtrip() {
        write_then_read_one();
    }

    #[test]
    fn tampered_payload_fails_read() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("v.tockvault");
        let v = init(&path, b"pw").expect("init");
        let device_id = v.local_device().device_id;
        let entity = Uuid::now_v7();
        let log = EventLog::new(&v);
        log.append(
            EntityKind::Task,
            entity,
            EventOp::Create,
            b"payload",
            VectorClock::singleton(tock_core::event::DeviceId::from_bytes(device_id), 1),
            None,
        )
        .expect("append");

        // Mutate the payload_ct out-of-band — simulates disk tamper.
        v.connection()
            .execute(
                "UPDATE events SET payload_ct = ?1",
                rusqlite::params![vec![0_u8; 32]],
            )
            .expect("tamper");
        let read = log.read_all();
        assert!(matches!(
            read,
            Err(Error::EventLogIntegrity | Error::InvalidVaultOrCredentials)
        ));
    }

    #[test]
    fn plaintext_absent_from_disk_after_append() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("v.tockvault");
        let v = init(&path, b"pw").expect("init");
        let device_id = v.local_device().device_id;
        let entity = Uuid::now_v7();
        let log = EventLog::new(&v);
        let marker: &[u8] = b"PLAINTEXT-CANARY-uuid-1f3b-9c2a";
        log.append(
            EntityKind::Task,
            entity,
            EventOp::Create,
            marker,
            VectorClock::singleton(tock_core::event::DeviceId::from_bytes(device_id), 1),
            None,
        )
        .expect("append");

        // Force a write to disk.
        v.connection()
            .execute_batch("PRAGMA wal_checkpoint(FULL);")
            .expect("ckpt");
        v.lock();

        let raw = std::fs::read(&path).expect("read");
        assert!(
            !raw.windows(marker.len()).any(|w| w == marker),
            "plaintext marker found in raw vault bytes"
        );
    }
}
