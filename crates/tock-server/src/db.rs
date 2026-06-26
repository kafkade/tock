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
                account_id TEXT,
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
            );

            CREATE TABLE IF NOT EXISTS accounts (
                id         TEXT PRIMARY KEY,
                email      TEXT NOT NULL UNIQUE,
                api_token  TEXT NOT NULL UNIQUE,
                tier       TEXT NOT NULL DEFAULT 'free',
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS usage_tracking (
                account_id   TEXT PRIMARY KEY,
                bytes_stored INTEGER NOT NULL DEFAULT 0,
                event_count  INTEGER NOT NULL DEFAULT 0
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

    /// Resolve an account id from an API token.
    pub fn account_id_by_api_token(&self, api_token: &str) -> Result<Option<String>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        conn.query_row(
            "SELECT id FROM accounts WHERE api_token = ?1",
            params![api_token],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    /// Claim an unowned vault for an account, or verify existing ownership.
    pub fn claim_vault_for_account(
        &self,
        vault_id: &[u8; 16],
        account_id: &str,
    ) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let current: Option<Option<String>> = conn
            .query_row(
                "SELECT account_id FROM vaults WHERE id = ?1",
                params![vault_id.to_vec()],
                |row| row.get(0),
            )
            .optional()?;
        match current {
            None => return Err(Error::NotFound),
            Some(Some(existing)) if existing != account_id => {
                return Err(Error::Unauthorized("vault belongs to a different account"));
            }
            Some(Some(_)) => return Ok(()),
            Some(None) => {}
        }
        conn.execute(
            "UPDATE vaults SET account_id = ?2 WHERE id = ?1",
            params![vault_id.to_vec(), account_id],
        )?;
        Ok(())
    }

    /// Ensure the given account owns the vault.
    pub fn require_vault_access(&self, vault_id: &[u8; 16], account_id: &str) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let owner: Option<Option<String>> = conn
            .query_row(
                "SELECT account_id FROM vaults WHERE id = ?1",
                params![vault_id.to_vec()],
                |row| row.get(0),
            )
            .optional()?;
        match owner {
            None => Err(Error::NotFound),
            Some(Some(existing)) if existing == account_id => Ok(()),
            Some(Some(_)) => Err(Error::Unauthorized("vault belongs to a different account")),
            Some(None) => Err(Error::Unauthorized(
                "vault is not yet associated with an account",
            )),
        }
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

    /// Pull events for a vault after a given opaque cursor position.
    ///
    /// The cursor is the server-assigned `rowid`, which is monotonic in
    /// insertion order. This is deliberately **not** the lamport value:
    /// lamports are per-device and an offline device can push events
    /// with lamports below ones already stored, which a lamport-ordered
    /// cursor would skip. Rowid ordering guarantees every event is
    /// delivered exactly once as the cursor advances.
    pub fn pull_events(
        &self,
        vault_id: &[u8; 16],
        after_cursor: i64,
        limit: usize,
    ) -> Result<Vec<StoredEvent>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let limit_i = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = conn.prepare(
            "SELECT rowid, id, device_id, lamport, payload, created_at
             FROM server_events
             WHERE vault_id = ?1 AND rowid > ?2
             ORDER BY rowid ASC
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![vault_id.to_vec(), after_cursor, limit_i], |row| {
            Ok(StoredEvent {
                rowid: row.get(0)?,
                id: row.get(1)?,
                device_id: row.get(2)?,
                lamport: row.get(3)?,
                payload: row.get(4)?,
                created_at: row.get(5)?,
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

    // ── Account management (hosted mode) ─────────────────────────────

    /// Create a new account. Returns `(account_id, api_token)`.
    pub fn create_account(
        &self,
        email: &str,
        tier: crate::billing::Tier,
    ) -> Result<(String, String), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let account_id = uuid::Uuid::now_v7().to_string();
        let api_token = format!("tok_{}", uuid::Uuid::now_v7().as_hyphenated());
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        conn.execute(
            "INSERT INTO accounts (id, email, api_token, tier, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![account_id, email, api_token, tier.as_str(), now],
        )?;
        conn.execute(
            "INSERT INTO usage_tracking (account_id) VALUES (?1)",
            params![account_id],
        )?;
        Ok((account_id, api_token))
    }

    /// Get account info: `(email, tier, usage)`.
    pub fn get_account(
        &self,
        account_id: &str,
    ) -> Result<Option<(String, String, crate::billing::UsageSnapshot)>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let row: Option<(String, String)> = conn
            .query_row(
                "SELECT email, tier FROM accounts WHERE id = ?1",
                params![account_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((email, tier)) = row else {
            return Ok(None);
        };
        let usage = Self::get_usage_inner(&conn, account_id);
        Ok(Some((email, tier, usage)))
    }

    /// Get usage snapshot for an account.
    pub fn get_usage(
        &self,
        account_id: &str,
    ) -> Result<Option<crate::billing::UsageSnapshot>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM accounts WHERE id = ?1",
                params![account_id],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if !exists {
            return Ok(None);
        }
        Ok(Some(Self::get_usage_inner(&conn, account_id)))
    }

    fn get_usage_inner(conn: &Connection, account_id: &str) -> crate::billing::UsageSnapshot {
        let (bytes_stored, event_count): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(bytes_stored, 0), COALESCE(event_count, 0)
                 FROM usage_tracking WHERE account_id = ?1",
                params![account_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));
        crate::billing::UsageSnapshot {
            bytes_stored: u64::try_from(bytes_stored).unwrap_or(0),
            event_count: u64::try_from(event_count).unwrap_or(0),
            device_count: 0,
            vault_count: 0,
        }
    }

    /// Increment usage counters after a push.
    #[allow(dead_code)]
    pub fn track_usage(
        &self,
        account_id: &str,
        bytes_delta: i64,
        events_delta: i64,
    ) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        conn.execute(
            "UPDATE usage_tracking
             SET bytes_stored = bytes_stored + ?2,
                 event_count  = event_count + ?3
             WHERE account_id = ?1",
            params![account_id, bytes_delta, events_delta],
        )?;
        Ok(())
    }
}

/// An event stored on the server.
pub struct StoredEvent {
    /// Server-assigned monotonic rowid (pull cursor position).
    pub rowid: i64,
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

    #[test]
    fn account_create_and_get() {
        let db = ServerDb::open_memory().expect("open");
        let (aid, token) = db
            .create_account("user@example.com", crate::billing::Tier::Personal)
            .expect("create");
        assert!(!aid.is_empty());
        assert!(token.starts_with("tok_"));

        let info = db.get_account(&aid).expect("get").expect("found");
        assert_eq!(info.0, "user@example.com");
        assert_eq!(info.1, "personal");
        assert_eq!(info.2.bytes_stored, 0);
    }

    #[test]
    fn usage_tracking_increments() {
        let db = ServerDb::open_memory().expect("open");
        let (aid, _) = db
            .create_account("u@e.com", crate::billing::Tier::Free)
            .expect("create");

        db.track_usage(&aid, 1024, 5).expect("track");
        db.track_usage(&aid, 2048, 3).expect("track2");

        let usage = db.get_usage(&aid).expect("get").expect("found");
        assert_eq!(usage.bytes_stored, 3072);
        assert_eq!(usage.event_count, 8);
    }

    #[test]
    fn hosted_account_can_claim_and_reuse_vault() {
        let db = ServerDb::open_memory().expect("open");
        let (aid, token) = db
            .create_account("user@example.com", crate::billing::Tier::Personal)
            .expect("create");
        let vault = [9_u8; 16];
        db.ensure_vault(&vault).expect("vault");

        let looked_up = db
            .account_id_by_api_token(&token)
            .expect("lookup")
            .expect("present");
        assert_eq!(looked_up, aid);

        db.claim_vault_for_account(&vault, &aid).expect("claim");
        db.require_vault_access(&vault, &aid).expect("access");
        db.claim_vault_for_account(&vault, &aid).expect("reclaim");
    }

    #[test]
    fn hosted_account_cannot_steal_other_vault() {
        let db = ServerDb::open_memory().expect("open");
        let (aid1, _) = db
            .create_account("user1@example.com", crate::billing::Tier::Personal)
            .expect("create 1");
        let (aid2, _) = db
            .create_account("user2@example.com", crate::billing::Tier::Personal)
            .expect("create 2");
        let vault = [10_u8; 16];
        db.ensure_vault(&vault).expect("vault");
        db.claim_vault_for_account(&vault, &aid1).expect("claim");
        assert!(matches!(
            db.require_vault_access(&vault, &aid2),
            Err(Error::Unauthorized(_))
        ));
        assert!(matches!(
            db.claim_vault_for_account(&vault, &aid2),
            Err(Error::Unauthorized(_))
        ));
    }
}
