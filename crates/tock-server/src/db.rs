//! Server-side `SQLite` storage.
//!
//! The server stores only opaque encrypted blobs. It indexes event
//! metadata for efficient pull queries but never accesses encrypted
//! payloads.

use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::Mutex;

use crate::error::Error;

/// Server database wrapper.
pub struct ServerDb {
    conn: Mutex<Connection>,
}

#[allow(clippy::significant_drop_tightening)]
impl ServerDb {
    /// Open or create the server database at `path`.
    ///
    /// # Errors
    /// Returns [`Error::Db`] on `SQLite` failure.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory database (for testing).
    #[cfg(test)]
    pub fn open_memory() -> Result<Self, Error> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS vaults (
                id         BLOB PRIMARY KEY,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS server_devices (
                vault_id      BLOB NOT NULL,
                device_id     BLOB NOT NULL,
                verifying_key BLOB NOT NULL,
                label         TEXT,
                registered_at TEXT NOT NULL,
                revoked       INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (vault_id, device_id)
            );

            CREATE TABLE IF NOT EXISTS server_events (
                id         BLOB PRIMARY KEY,
                vault_id   BLOB NOT NULL,
                device_id  BLOB NOT NULL,
                lamport    INTEGER NOT NULL,
                payload    BLOB NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS server_events_vault_lamport
                ON server_events (vault_id, lamport);

            CREATE TABLE IF NOT EXISTS onboarding_blobs (
                vault_id      BLOB NOT NULL,
                target_device BLOB NOT NULL,
                blob          BLOB NOT NULL,
                created_at    TEXT NOT NULL,
                PRIMARY KEY (vault_id, target_device)
            );",
        )?;
        Ok(())
    }

    /// Register a vault if it doesn't exist.
    pub fn ensure_vault(&self, vault_id: &[u8; 16]) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        conn.execute(
            "INSERT OR IGNORE INTO vaults (id, created_at) VALUES (?1, ?2)",
            params![vault_id.to_vec(), now],
        )?;
        Ok(())
    }

    /// Register a device for a vault.
    pub fn register_device(
        &self,
        vault_id: &[u8; 16],
        device_id: &[u8; 16],
        verifying_key: &[u8; 32],
        label: Option<&str>,
    ) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        conn.execute(
            "INSERT OR REPLACE INTO server_devices
             (vault_id, device_id, verifying_key, label, registered_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                vault_id.to_vec(),
                device_id.to_vec(),
                verifying_key.to_vec(),
                label,
                now
            ],
        )?;
        Ok(())
    }

    /// Look up a device's verifying key.
    #[allow(dead_code)]
    pub fn get_device_key(
        &self,
        vault_id: &[u8; 16],
        device_id: &[u8; 16],
    ) -> Result<Option<Vec<u8>>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let key: Option<Vec<u8>> = conn
            .query_row(
                "SELECT verifying_key FROM server_devices
                 WHERE vault_id = ?1 AND device_id = ?2 AND revoked = 0",
                params![vault_id.to_vec(), device_id.to_vec()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(key)
    }

    /// Push an opaque event blob.
    pub fn push_event(
        &self,
        event_id: &[u8; 16],
        vault_id: &[u8; 16],
        device_id: &[u8; 16],
        lamport: i64,
        payload: &[u8],
    ) -> Result<bool, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let result = conn.execute(
            "INSERT OR IGNORE INTO server_events
             (id, vault_id, device_id, lamport, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event_id.to_vec(),
                vault_id.to_vec(),
                device_id.to_vec(),
                lamport,
                payload,
                now
            ],
        )?;
        Ok(result > 0)
    }

    /// Pull events for a vault after a given lamport value.
    pub fn pull_events(
        &self,
        vault_id: &[u8; 16],
        after_lamport: i64,
        limit: usize,
    ) -> Result<Vec<StoredEvent>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let limit_i = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = conn.prepare(
            "SELECT id, device_id, lamport, payload, created_at
             FROM server_events
             WHERE vault_id = ?1 AND lamport > ?2
             ORDER BY lamport ASC
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![vault_id.to_vec(), after_lamport, limit_i], |row| {
            Ok(StoredEvent {
                id: row.get(0)?,
                device_id: row.get(1)?,
                lamport: row.get(2)?,
                payload: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    /// Store an onboarding blob.
    pub fn put_onboarding_blob(
        &self,
        vault_id: &[u8; 16],
        target_device: &[u8; 16],
        blob: &[u8],
    ) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        conn.execute(
            "INSERT OR REPLACE INTO onboarding_blobs
             (vault_id, target_device, blob, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![vault_id.to_vec(), target_device.to_vec(), blob, now],
        )?;
        Ok(())
    }

    /// Retrieve an onboarding blob.
    pub fn get_onboarding_blob(
        &self,
        vault_id: &[u8; 16],
        target_device: &[u8; 16],
    ) -> Result<Option<Vec<u8>>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let blob: Option<Vec<u8>> = conn
            .query_row(
                "SELECT blob FROM onboarding_blobs
                 WHERE vault_id = ?1 AND target_device = ?2",
                params![vault_id.to_vec(), target_device.to_vec()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(blob)
    }

    /// Get the maximum lamport value for a vault.
    pub fn max_lamport(&self, vault_id: &[u8; 16]) -> Result<i64, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let max: Option<i64> = conn
            .query_row(
                "SELECT MAX(lamport) FROM server_events WHERE vault_id = ?1",
                params![vault_id.to_vec()],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(max.unwrap_or(0))
    }
}

/// An event stored on the server.
pub struct StoredEvent {
    /// Event id bytes.
    pub id: Vec<u8>,
    /// Device that produced the event.
    pub device_id: Vec<u8>,
    /// Lamport timestamp.
    pub lamport: i64,
    /// Opaque encrypted payload (full wire-format event frame).
    pub payload: Vec<u8>,
    /// When the server received the event.
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn db_roundtrip() {
        let db = ServerDb::open_memory().expect("open");
        let vault = [1_u8; 16];
        let device = [2_u8; 16];
        let vk = [3_u8; 32];
        let event_id = [4_u8; 16];

        db.ensure_vault(&vault).expect("vault");
        db.register_device(&vault, &device, &vk, Some("test"))
            .expect("device");

        let key = db.get_device_key(&vault, &device).expect("key");
        assert_eq!(key.as_deref(), Some(vk.as_slice()));

        let inserted = db
            .push_event(&event_id, &vault, &device, 1, b"opaque-blob")
            .expect("push");
        assert!(inserted);

        // Duplicate is ignored.
        let dup = db
            .push_event(&event_id, &vault, &device, 1, b"opaque-blob")
            .expect("push dup");
        assert!(!dup);

        let events = db.pull_events(&vault, 0, 100).expect("pull");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload, b"opaque-blob");
        assert_eq!(events[0].lamport, 1);

        assert_eq!(db.max_lamport(&vault).expect("max"), 1);
    }

    #[test]
    fn onboarding_blob_roundtrip() {
        let db = ServerDb::open_memory().expect("open");
        let vault = [1_u8; 16];
        let target = [5_u8; 16];

        db.ensure_vault(&vault).expect("vault");
        db.put_onboarding_blob(&vault, &target, b"encrypted-vk")
            .expect("put");
        let blob = db.get_onboarding_blob(&vault, &target).expect("get");
        assert_eq!(blob.as_deref(), Some(b"encrypted-vk".as_slice()));
    }

    #[test]
    fn pull_respects_after_lamport() {
        let db = ServerDb::open_memory().expect("open");
        let vault = [1_u8; 16];
        let device = [2_u8; 16];

        db.ensure_vault(&vault).expect("vault");
        for i in 1..=5 {
            let mut eid = [0_u8; 16];
            eid[0] = i;
            db.push_event(&eid, &vault, &device, i64::from(i), &[i])
                .expect("push");
        }

        let after_3 = db.pull_events(&vault, 3, 100).expect("pull");
        assert_eq!(after_3.len(), 2); // lamport 4 and 5
        assert_eq!(after_3[0].lamport, 4);
        assert_eq!(after_3[1].lamport, 5);
    }
}
