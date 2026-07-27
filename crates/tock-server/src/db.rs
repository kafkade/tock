//! Server-side `SQLite` storage.
//!
//! The server stores only opaque encrypted blobs. It indexes event
//! metadata for efficient pull queries but never accesses encrypted
//! payloads.

use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::Mutex;

use crate::error::Error;

/// One row of [`ServerDb::list_vaults`]: a vault id and its owning account id
/// (`None` when the vault is unclaimed).
pub type VaultListing = (Vec<u8>, Option<String>);

/// Server database wrapper.
pub struct ServerDb {
    conn: Mutex<Connection>,
}

/// One numbered schema migration, applied in order and tracked via
/// `PRAGMA user_version` (mirroring the `tock-storage` migration approach).
struct Migration {
    /// Monotonic version (matches `PRAGMA user_version` after apply).
    version: u32,
    /// SQL executed in a single transaction.
    sql: &'static str,
}

/// All server migrations, in apply order.
///
/// - **v1** is the historical schema (idempotent `CREATE TABLE IF NOT EXISTS`)
///   so pre-existing on-disk databases — which were created before
///   `user_version` tracking — converge cleanly.
/// - **v2** introduces the self-hosted account system (ADR-011 / issue #127):
///   it rebuilds `accounts` with SRP-verifier storage, roles, and status, and
///   adds `server_settings` (registration policy) and `account_invites`.
/// - **v3** adds `sessions` for authenticated sync (issue #130): SRP login
///   issues short-lived bearer tokens (derived from the session key `K`); only
///   their hash + channel-binding tag + expiry are persisted.
/// - **v4** adds `vault_headers` (issue #129): the non-secret vault header
///   (KDF salts/params + MEK-wrapped Vault Key) so a fresh device can recover
///   the vault after SRP login without device-to-device pairing.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: "CREATE TABLE IF NOT EXISTS vaults (
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
    },
    Migration {
        version: 2,
        // Rebuild `accounts` into the self-hosted account model. The server
        // stores SRP verifiers ONLY — never a password, Secret Key, or 2SKD
        // root (ADR-011). `api_token` is now nullable: it carries the hosted
        // billing bearer token AND the interim admin bearer token (issue #130
        // replaces the latter with SRP session tokens). Existing billing rows
        // are preserved as `role = 'user'`, `status = 'active'`.
        sql: "ALTER TABLE accounts RENAME TO accounts_legacy;

            CREATE TABLE accounts (
                id           TEXT PRIMARY KEY,
                username     TEXT NOT NULL UNIQUE,
                srp_salt     BLOB,
                srp_verifier BLOB,
                srp_group    TEXT,
                kdf_params   TEXT,
                role         TEXT NOT NULL DEFAULT 'user',
                status       TEXT NOT NULL DEFAULT 'active',
                api_token    TEXT UNIQUE,
                tier         TEXT NOT NULL DEFAULT 'free',
                created_at   TEXT NOT NULL
            );

            INSERT INTO accounts
                (id, username, api_token, tier, role, status, created_at)
            SELECT id, email, api_token, tier, 'user', 'active', created_at
            FROM accounts_legacy;

            DROP TABLE accounts_legacy;

            CREATE TABLE server_settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE account_invites (
                token      TEXT PRIMARY KEY,
                username   TEXT,
                role       TEXT NOT NULL DEFAULT 'user',
                created_at TEXT NOT NULL,
                used       INTEGER NOT NULL DEFAULT 0
            );",
    },
    Migration {
        version: 3,
        // Authenticated-sync session tokens (issue #130). A row exists for each
        // live SRP login. We persist only a SHA-256 *hash* of the bearer token —
        // which is itself HKDF-derived from the SRP session key `K`, so a DB
        // leak yields no usable token — plus the non-secret channel-binding tag
        // and an absolute expiry (Unix seconds). No password, Secret Key, URK,
        // or `K` is ever stored.
        sql: "CREATE TABLE sessions (
                token_hash      TEXT PRIMARY KEY,
                account_id      TEXT NOT NULL,
                channel_binding BLOB NOT NULL,
                created_at      TEXT NOT NULL,
                expires_at      INTEGER NOT NULL
            );
            CREATE INDEX sessions_expires_at ON sessions (expires_at);
            CREATE INDEX sessions_account_id ON sessions (account_id);",
    },
    Migration {
        version: 4,
        // Vault-header bootstrap for new-device account login (issue #129).
        // The vault header is NON-secret: it carries KDF salts/params and the
        // Vault Key *wrapped* under MEK←URK (password + Secret Key), so the
        // server still never sees plaintext keys. Storing it lets a fresh
        // device sign in with only email+password+Secret Key (no second device
        // online) and recover the vault without device-to-device pairing.
        sql: "CREATE TABLE vault_headers (
                vault_id   BLOB PRIMARY KEY,
                header     BLOB NOT NULL,
                updated_at TEXT NOT NULL
            );",
    },
];

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
        let current: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for migration in MIGRATIONS {
            if migration.version <= current {
                continue;
            }
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(migration.sql)?;
            // `PRAGMA user_version` does not accept bound parameters.
            tx.execute_batch(&format!("PRAGMA user_version = {};", migration.version))?;
            tx.commit()?;
        }
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
                return Err(Error::Forbidden("vault belongs to a different account"));
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
            Some(Some(_)) => Err(Error::Forbidden("vault belongs to a different account")),
            Some(None) => Err(Error::Forbidden(
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

    /// Read the **entire** event log for a vault, ordered by `rowid ASC`.
    ///
    /// Unlike [`pull_events`](Self::pull_events) (cursor-paged incremental sync),
    /// this returns every stored event in one pass for a point-in-time
    /// ciphertext **export** (issue #201). Payloads are returned opaquely; the
    /// server never decrypts.
    pub fn all_events(&self, vault_id: &[u8; 16]) -> Result<Vec<StoredEvent>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT rowid, id, device_id, lamport, payload, created_at
             FROM server_events
             WHERE vault_id = ?1
             ORDER BY rowid ASC",
        )?;
        let rows = stmt.query_map(params![vault_id.to_vec()], |row| {
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

    /// Store (or replace) the non-secret vault header blob for a vault.
    ///
    /// The header carries KDF salts/params and the MEK-wrapped Vault Key; the
    /// server never decrypts it. Uploaded by the owning device at signup so a
    /// new device can recover the vault after SRP login (issue #129).
    pub fn put_vault_header(&self, vault_id: &[u8; 16], header: &[u8]) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        conn.execute(
            "INSERT OR REPLACE INTO vault_headers (vault_id, header, updated_at)
             VALUES (?1, ?2, ?3)",
            params![vault_id.to_vec(), header, now],
        )?;
        Ok(())
    }

    /// Fetch the stored vault header blob for a vault, if any.
    pub fn get_vault_header(&self, vault_id: &[u8; 16]) -> Result<Option<Vec<u8>>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let header: Option<Vec<u8>> = conn
            .query_row(
                "SELECT header FROM vault_headers WHERE vault_id = ?1",
                params![vault_id.to_vec()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(header)
    }

    /// Fetch the stored header for whichever vault the account owns. Used by a
    /// fresh device that knows its account but not yet its vault id
    /// (issue #129 new-device login). Returns the most-recently updated header
    /// if several vaults are claimed by the account.
    pub fn get_vault_header_for_account(&self, account_id: &str) -> Result<Option<Vec<u8>>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let header: Option<Vec<u8>> = conn
            .query_row(
                "SELECT h.header FROM vault_headers h
                 JOIN vaults v ON v.id = h.vault_id
                 WHERE v.account_id = ?1
                 ORDER BY h.updated_at DESC LIMIT 1",
                params![account_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(header)
    }

    /// Replace the stored vault header for the account's vault after a
    /// password rotation re-wraps the Vault Key (issue #131). Scoped to the
    /// owning account; returns `false` if the account has no stored header yet
    /// (e.g. a browser-only account that never uploaded one).
    pub fn update_vault_header_for_account(
        &self,
        account_id: &str,
        header: &[u8],
    ) -> Result<bool, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let updated = conn.execute(
            "UPDATE vault_headers
                SET header = ?2, updated_at = ?3
              WHERE vault_id IN (SELECT id FROM vaults WHERE account_id = ?1)",
            params![account_id, header, rfc3339_now()],
        )?;
        Ok(updated > 0)
    }
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

    // ── Export enumeration + snapshot (issue #201) ───────────────────

    /// List every vault as `(vault_id_bytes, account_id)` for the offline
    /// admin `--all` export. `account_id` is `None` for an unclaimed vault.
    /// Returns only opaque ids — no ciphertext is read.
    pub fn list_vaults(&self) -> Result<Vec<VaultListing>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT id, account_id FROM vaults ORDER BY id ASC")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut vaults = Vec::new();
        for row in rows {
            vaults.push(row?);
        }
        Ok(vaults)
    }

    /// List the vault ids claimed by `account_id`, for the offline admin
    /// `--account <id>` export.
    pub fn vaults_for_account(&self, account_id: &str) -> Result<Vec<[u8; 16]>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let mut stmt =
            conn.prepare("SELECT id FROM vaults WHERE account_id = ?1 ORDER BY id ASC")?;
        let rows = stmt.query_map(params![account_id], |row| row.get::<_, Vec<u8>>(0))?;
        let mut vaults = Vec::new();
        for row in rows {
            let bytes = row?;
            let arr: [u8; 16] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| Error::Internal("stored vault id is not 16 bytes".into()))?;
            vaults.push(arr);
        }
        Ok(vaults)
    }

    /// Produce a consistent point-in-time snapshot of the whole database at
    /// `dest` (which must not already exist) using `SQLite` `VACUUM INTO`.
    ///
    /// This is the primitive behind the scheduled server-retained snapshots
    /// (issue #201). It is consistent even under WAL journaling and mirrors
    /// `tock-storage`'s client-side backup path. The snapshot is ciphertext
    /// only — the server never decrypts.
    pub fn snapshot_to(&self, dest: &Path) -> Result<(), Error> {
        let dest_str = dest
            .to_str()
            .ok_or_else(|| Error::Internal("snapshot path is not valid UTF-8".into()))?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        conn.execute("VACUUM INTO ?1", params![dest_str])?;
        Ok(())
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
            "INSERT INTO accounts (id, username, api_token, tier, role, status, created_at)
             VALUES (?1, ?2, ?3, ?4, 'user', 'active', ?5)",
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
                "SELECT username, tier FROM accounts WHERE id = ?1",
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

    // ── Self-hosted account system (issue #127 / ADR-011) ────────────

    /// Total number of accounts. Zero means the instance is unbootstrapped.
    pub fn account_count(&self) -> Result<i64, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
        Ok(count)
    }

    /// Register a new self-hosted account, storing its SRP verifier material.
    ///
    /// Enforces the supplied registration `policy` and consumes an invite when
    /// one is required. The first account on a fresh instance is bootstrapped
    /// as an `admin` (Immich pattern), bypassing the policy. When the resulting
    /// account is an admin, an interim admin bearer token is minted and
    /// returned (issue #130 replaces this with SRP session tokens).
    ///
    /// The server never receives a password, Secret Key, URK, or `x` — only the
    /// public verifier and its salt/parameters.
    /// Convenience wrapper around [`Self::adopt_register`] with no vault claim
    /// (fresh registration). Retained for the unit tests that exercise the
    /// account-registration path directly.
    #[cfg(test)]
    pub fn register_account(
        &self,
        new: &NewAccount<'_>,
        policy: crate::accounts::RegistrationPolicy,
    ) -> Result<RegisterOutcome, Error> {
        self.adopt_register(new, policy, None)
    }

    /// Register an account and, when `vault` is supplied, claim the vault
    /// bucket and store its wrapped header — all in **one** database
    /// transaction (ADR-016 §5). This is the register-and-claim path behind
    /// both `signup` (fresh vault) and `adopt` (existing local vault): a
    /// partial failure consumes no username/invite and leaves no half-bound
    /// vault, so the operation is retry-safe.
    ///
    /// `vault` is `(vault_id, wrapped_header)`; the header is opaque — the
    /// server never decrypts it and never mints the vault's crypto identity
    /// (**A**/**V** stay client-owned). Full normalization and the opaque
    /// per-server alias are tightened in issue #199.
    pub fn adopt_register(
        &self,
        new: &NewAccount<'_>,
        policy: crate::accounts::RegistrationPolicy,
        vault: Option<(&[u8; 16], &[u8])>,
    ) -> Result<RegisterOutcome, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let tx = conn.unchecked_transaction()?;

        let outcome = Self::register_account_tx(&tx, new, policy)?;

        if let Some((vault_id, header)) = vault {
            Self::ensure_and_claim_vault_tx(&tx, vault_id, &outcome.account_id)?;
            Self::put_vault_header_tx(&tx, vault_id, header)?;
        }

        tx.commit()?;
        Ok(outcome)
    }

    /// Account-creation body of [`Self::adopt_register`], operating on an open
    /// transaction so the optional vault-claim + header-store commit or roll
    /// back together with it.
    fn register_account_tx(
        tx: &Connection,
        new: &NewAccount<'_>,
        policy: crate::accounts::RegistrationPolicy,
    ) -> Result<RegisterOutcome, Error> {
        use crate::accounts::RegistrationPolicy;

        // Canonicalize the identifier once (issue #199 / ADR-016 §5): store and
        // uniqueness-check the normalized form so case- and Unicode-equivalent
        // spellings (`Alice` vs `alice`, full-width, combining sequences) map to
        // a single account. The SRP verifier is username-independent and the
        // login M1 identity binds the raw wire string per-handshake, so
        // normalizing storage/lookup here does not affect the SRP proof.
        let username = crate::identifier::normalize(new.username);

        // An already-registered username is a conflict, not a policy failure —
        // check it first so a re-registration is a deterministic 409 regardless
        // of the registration policy or invite state. This keeps register (and
        // therefore `adopt`) idempotent and retry-safe (ADR-016 §5); a second
        // device re-adopting drives this 409 → SRP-login + verify path.
        let exists: bool = tx.query_row(
            "SELECT COUNT(*) > 0 FROM accounts WHERE username = ?1",
            params![username],
            |r| r.get(0),
        )?;
        if exists {
            return Err(Error::Conflict(format!(
                "username already registered: {username}"
            )));
        }

        let count: i64 = tx.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
        let bootstrap = count == 0;

        let role = if bootstrap {
            "admin".to_string()
        } else {
            match policy {
                RegistrationPolicy::Open => match new.invite_token {
                    Some(token) => Self::consume_invite_tx(tx, token, &username)?,
                    None => "user".to_string(),
                },
                RegistrationPolicy::InviteOnly | RegistrationPolicy::Disabled => {
                    let token = new
                        .invite_token
                        .ok_or(Error::Forbidden("registration requires a valid invite"))?;
                    Self::consume_invite_tx(tx, token, &username)?
                }
            }
        };

        let account_id = uuid::Uuid::now_v7().to_string();
        let now = rfc3339_now();
        let admin_token = if role == "admin" {
            Some(format!("adm_{}", uuid::Uuid::now_v7().as_hyphenated()))
        } else {
            None
        };

        tx.execute(
            "INSERT INTO accounts
               (id, username, srp_salt, srp_verifier, srp_group, kdf_params,
                role, status, api_token, tier, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8, 'free', ?9)",
            params![
                account_id,
                username,
                new.srp_salt,
                new.srp_verifier,
                new.srp_group,
                new.kdf_params,
                role,
                admin_token,
                now,
            ],
        )?;

        Ok(RegisterOutcome {
            account_id,
            role,
            status: "active".to_string(),
            admin_token,
        })
    }

    /// Transaction-scoped `ensure_vault` + `claim_vault_for_account`: create
    /// the bucket if absent, then claim it for `account_id` or verify existing
    /// ownership. Rejects a vault already owned by a different account so a
    /// second, unrelated adoption of the same `vault_id` cannot succeed.
    fn ensure_and_claim_vault_tx(
        tx: &Connection,
        vault_id: &[u8; 16],
        account_id: &str,
    ) -> Result<(), Error> {
        tx.execute(
            "INSERT OR IGNORE INTO vaults (id, created_at) VALUES (?1, ?2)",
            params![vault_id.to_vec(), rfc3339_now()],
        )?;
        let current: Option<Option<String>> = tx
            .query_row(
                "SELECT account_id FROM vaults WHERE id = ?1",
                params![vault_id.to_vec()],
                |row| row.get(0),
            )
            .optional()?;
        match current {
            None => return Err(Error::NotFound),
            Some(Some(existing)) if existing != account_id => {
                return Err(Error::Forbidden("vault belongs to a different account"));
            }
            Some(Some(_)) => return Ok(()),
            Some(None) => {}
        }
        tx.execute(
            "UPDATE vaults SET account_id = ?2 WHERE id = ?1",
            params![vault_id.to_vec(), account_id],
        )?;
        Ok(())
    }

    /// Transaction-scoped `put_vault_header`.
    fn put_vault_header_tx(
        tx: &Connection,
        vault_id: &[u8; 16],
        header: &[u8],
    ) -> Result<(), Error> {
        tx.execute(
            "INSERT OR REPLACE INTO vault_headers (vault_id, header, updated_at)
             VALUES (?1, ?2, ?3)",
            params![vault_id.to_vec(), header, rfc3339_now()],
        )?;
        Ok(())
    }

    /// Revoke the server principal **B** for `disconnect` (ADR-016 §3): in one
    /// transaction, disown the account's vault buckets (so **V** returns to
    /// unowned and is re-adoptable), delete its sessions, and delete the
    /// account row. The encrypted events and header remain untouched; **A**,
    /// **V**, and all local data live only on the client and are unaffected.
    /// Returns `false` if no such account exists.
    pub fn disconnect_account(&self, account_id: &str) -> Result<bool, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE vaults SET account_id = NULL WHERE account_id = ?1",
            params![account_id],
        )?;
        tx.execute(
            "DELETE FROM sessions WHERE account_id = ?1",
            params![account_id],
        )?;
        let deleted = tx.execute("DELETE FROM accounts WHERE id = ?1", params![account_id])?;
        tx.commit()?;
        Ok(deleted > 0)
    }

    /// Validate and consume an invite token, returning the role it grants.
    fn consume_invite_tx(tx: &Connection, token: &str, username: &str) -> Result<String, Error> {
        let row: Option<(Option<String>, String, i64)> = tx
            .query_row(
                "SELECT username, role, used FROM account_invites WHERE token = ?1",
                params![token],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (pinned, role, used) = row.ok_or(Error::Forbidden("invalid invite token"))?;
        if used != 0 {
            return Err(Error::Forbidden("invite token already used"));
        }
        if pinned.is_some_and(|p| p != username) {
            return Err(Error::Forbidden(
                "invite token does not match this username",
            ));
        }
        tx.execute(
            "UPDATE account_invites SET used = 1 WHERE token = ?1",
            params![token],
        )?;
        Ok(role)
    }

    /// Create an invite, optionally pinned to a `username`, granting `role`.
    /// Returns the opaque invite token.
    ///
    /// The pinned username is stored in its normalized form (issue #199) so it
    /// compares equal to the normalized identifier a client later registers
    /// with (see [`Self::consume_invite_tx`]).
    pub fn create_invite(&self, username: Option<&str>, role: &str) -> Result<String, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let pinned = username.map(crate::identifier::normalize);
        let token = format!("inv_{}", uuid::Uuid::now_v7().as_hyphenated());
        conn.execute(
            "INSERT INTO account_invites (token, username, role, created_at, used)
             VALUES (?1, ?2, ?3, ?4, 0)",
            params![token, pinned, role, rfc3339_now()],
        )?;
        Ok(token)
    }

    /// List all accounts (no secret material) for admin endpoints / CLI.
    pub fn list_users(&self) -> Result<Vec<UserRecord>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, username, role, status, created_at
             FROM accounts ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(UserRecord {
                id: r.get(0)?,
                username: r.get(1)?,
                role: r.get(2)?,
                status: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?;
        let mut users = Vec::new();
        for row in rows {
            users.push(row?);
        }
        Ok(users)
    }

    /// Set an account's status (`active` / `disabled`). Returns `false` if no
    /// such account exists.
    pub fn set_user_status(&self, account_id: &str, status: &str) -> Result<bool, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let updated = conn.execute(
            "UPDATE accounts SET status = ?2 WHERE id = ?1",
            params![account_id, status],
        )?;
        Ok(updated > 0)
    }

    /// Delete an account. Returns `false` if no such account exists.
    pub fn delete_user(&self, account_id: &str) -> Result<bool, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let deleted = conn.execute("DELETE FROM accounts WHERE id = ?1", params![account_id])?;
        Ok(deleted > 0)
    }

    /// Resolve an active admin account id from its interim admin bearer token.
    ///
    /// This is the authorization seam for the admin API. Issue #130 will extend
    /// admin authorization to accept SRP session tokens; the admin endpoints
    /// themselves do not change.
    pub fn admin_account_id_by_token(&self, token: &str) -> Result<Option<String>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        conn.query_row(
            "SELECT id FROM accounts
             WHERE api_token = ?1 AND role = 'admin' AND status = 'active'",
            params![token],
            |r| r.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    /// The current registration policy (defaults to `disabled`).
    pub fn registration_policy(&self) -> Result<crate::accounts::RegistrationPolicy, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM server_settings WHERE key = 'registration_policy'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(value
            .as_deref()
            .and_then(crate::accounts::RegistrationPolicy::from_str_opt)
            .unwrap_or(crate::accounts::RegistrationPolicy::Disabled))
    }

    /// Persist the registration policy.
    pub fn set_registration_policy(
        &self,
        policy: crate::accounts::RegistrationPolicy,
    ) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO server_settings (key, value) VALUES ('registration_policy', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![policy.as_str()],
        )?;
        Ok(())
    }

    /// Seam for issue #130's SRP login: fetch the stored verifier material for
    /// a username. Disabled accounts and accounts without SRP credentials
    /// (e.g. hosted billing rows) are excluded.
    /// Seam for issue #130's SRP login: fetch the stored verifier material for
    /// a username. Disabled accounts and accounts without SRP credentials
    /// (e.g. hosted billing rows) are excluded.
    pub fn get_srp_credentials(&self, username: &str) -> Result<Option<SrpCredentials>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        // Look up by the normalized identifier (issue #199) so a login succeeds
        // regardless of the case/Unicode spelling the client sent, matching the
        // form stored at registration.
        let username = crate::identifier::normalize(username);
        conn.query_row(
            "SELECT id, srp_salt, srp_verifier, srp_group, kdf_params FROM accounts
             WHERE username = ?1 AND status = 'active'
               AND srp_salt IS NOT NULL AND srp_verifier IS NOT NULL",
            params![username],
            |r| {
                Ok(SrpCredentials {
                    account_id: r.get::<_, String>(0)?,
                    srp_salt: r.get::<_, Vec<u8>>(1)?,
                    srp_verifier: r.get::<_, Vec<u8>>(2)?,
                    srp_group: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    kdf_params: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    // ── Authenticated-sync sessions (issue #130) ─────────────────────────

    /// Persist a freshly minted SRP login session. `token_hash` is the
    /// hex SHA-256 of the bearer token (which is itself HKDF-derived from the
    /// session key `K`); `channel_binding` is the non-secret AAD tag;
    /// `expires_at` is an absolute Unix-seconds deadline.
    pub fn create_session(
        &self,
        token_hash: &str,
        account_id: &str,
        channel_binding: &[u8],
        expires_at: i64,
    ) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        conn.execute(
            "INSERT OR REPLACE INTO sessions
               (token_hash, account_id, channel_binding, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                token_hash,
                account_id,
                channel_binding,
                rfc3339_now(),
                expires_at
            ],
        )?;
        Ok(())
    }

    /// Resolve a live session from its `token_hash`. Returns `None` if the
    /// session is unknown, expired (`expires_at <= now`), or its account is no
    /// longer active. `now` is Unix seconds (passed in so callers share one
    /// clock reading).
    pub fn lookup_session(
        &self,
        token_hash: &str,
        now: i64,
    ) -> Result<Option<SessionRecord>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        conn.query_row(
            "SELECT s.account_id, s.channel_binding, a.role
               FROM sessions s
               JOIN accounts a ON a.id = s.account_id
              WHERE s.token_hash = ?1 AND s.expires_at > ?2 AND a.status = 'active'",
            params![token_hash, now],
            |r| {
                Ok(SessionRecord {
                    account_id: r.get::<_, String>(0)?,
                    channel_binding: r.get::<_, Vec<u8>>(1)?,
                    role: r.get::<_, String>(2)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    /// Slide a live session's expiry forward. Returns `false` if the session is
    /// unknown or already expired (in which case the caller must re-login).
    pub fn refresh_session(
        &self,
        token_hash: &str,
        now: i64,
        new_expires_at: i64,
    ) -> Result<bool, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let updated = conn.execute(
            "UPDATE sessions SET expires_at = ?3
              WHERE token_hash = ?1 AND expires_at > ?2",
            params![token_hash, now, new_expires_at],
        )?;
        Ok(updated > 0)
    }

    /// Delete sessions whose expiry has passed. Best-effort housekeeping called
    /// opportunistically; returns the number of rows removed.
    pub fn delete_expired_sessions(&self, now: i64) -> Result<usize, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let deleted = conn.execute("DELETE FROM sessions WHERE expires_at <= ?1", params![now])?;
        Ok(deleted)
    }

    // ── Self-service: password rotation, sessions, devices (issue #131) ───

    /// Rotate an account's stored SRP verifier material after a client-side
    /// password change. Only accounts that already have SRP credentials are
    /// updated (hosted billing rows have none). Returns `false` if no such
    /// eligible account exists. The server never sees the password, Secret Key,
    /// or URK — only the freshly derived, non-secret verifier.
    pub fn update_srp_credentials(
        &self,
        account_id: &str,
        srp_salt: &[u8],
        srp_verifier: &[u8],
        srp_group: &str,
        kdf_params: &str,
    ) -> Result<bool, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let updated = conn.execute(
            "UPDATE accounts
                SET srp_salt = ?2, srp_verifier = ?3, srp_group = ?4, kdf_params = ?5
              WHERE id = ?1 AND srp_verifier IS NOT NULL",
            params![account_id, srp_salt, srp_verifier, srp_group, kdf_params],
        )?;
        Ok(updated > 0)
    }

    /// List an account's live (unexpired) sessions, newest first. The
    /// `token_hash` identifies each row so the caller can flag the current
    /// session and target a specific one for revocation.
    pub fn list_sessions(&self, account_id: &str, now: i64) -> Result<Vec<SessionInfo>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT token_hash, created_at, expires_at
               FROM sessions
              WHERE account_id = ?1 AND expires_at > ?2
              ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![account_id, now], |r| {
            Ok(SessionInfo {
                token_hash: r.get(0)?,
                created_at: r.get(1)?,
                expires_at: r.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Revoke a single session by its `token_hash`, scoped to the owning
    /// account so a caller can only end their own sessions. Returns `false` if
    /// no matching session exists.
    pub fn delete_session(&self, account_id: &str, token_hash: &str) -> Result<bool, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let deleted = conn.execute(
            "DELETE FROM sessions WHERE account_id = ?1 AND token_hash = ?2",
            params![account_id, token_hash],
        )?;
        Ok(deleted > 0)
    }

    /// Revoke every session for an account except `keep_token_hash` (typically
    /// the caller's current session). Returns the number of sessions ended.
    pub fn delete_sessions_except(
        &self,
        account_id: &str,
        keep_token_hash: &str,
    ) -> Result<usize, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let deleted = conn.execute(
            "DELETE FROM sessions WHERE account_id = ?1 AND token_hash <> ?2",
            params![account_id, keep_token_hash],
        )?;
        Ok(deleted)
    }

    /// List the devices registered across all of an account's vaults.
    pub fn list_devices_for_account(&self, account_id: &str) -> Result<Vec<DeviceInfo>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT d.device_id, d.label, d.registered_at, d.revoked
               FROM server_devices d
               JOIN vaults v ON v.id = d.vault_id
              WHERE v.account_id = ?1
              ORDER BY d.registered_at DESC",
        )?;
        let rows = stmt.query_map(params![account_id], |r| {
            Ok(DeviceInfo {
                device_id: r.get::<_, Vec<u8>>(0)?,
                label: r.get(1)?,
                registered_at: r.get(2)?,
                revoked: r.get::<_, i64>(3)? != 0,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Mark a device revoked, scoped to the owning account. Returns `false` if
    /// the device is not found among the account's vaults.
    pub fn revoke_device_for_account(
        &self,
        account_id: &str,
        device_id: &[u8; 16],
    ) -> Result<bool, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let updated = conn.execute(
            "UPDATE server_devices SET revoked = 1
              WHERE device_id = ?2
                AND vault_id IN (SELECT id FROM vaults WHERE account_id = ?1)",
            params![account_id, device_id.to_vec()],
        )?;
        Ok(updated > 0)
    }

    // ── Instance settings + stats (issue #131) ───────────────────────────

    /// The configured public server address, if the admin has set one.
    pub fn public_address(&self) -> Result<Option<String>, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        conn.query_row(
            "SELECT value FROM server_settings WHERE key = 'public_address'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map(|v| v.filter(|s| !s.is_empty()))
        .map_err(Into::into)
    }

    /// Persist (or clear, when empty) the public server address.
    pub fn set_public_address(&self, address: &str) -> Result<(), Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO server_settings (key, value) VALUES ('public_address', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![address],
        )?;
        Ok(())
    }

    /// Aggregate, non-secret instance statistics for the admin usage/health
    /// panel. All values are counts / byte totals over opaque data.
    pub fn instance_stats(&self) -> Result<InstanceStats, Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let scalar = |sql: &str| -> Result<i64, Error> {
            conn.query_row(sql, [], |r| r.get::<_, i64>(0))
                .map_err(Into::into)
        };
        Ok(InstanceStats {
            accounts_total: scalar("SELECT COUNT(*) FROM accounts")?,
            accounts_admin: scalar("SELECT COUNT(*) FROM accounts WHERE role = 'admin'")?,
            accounts_active: scalar("SELECT COUNT(*) FROM accounts WHERE status = 'active'")?,
            accounts_disabled: scalar("SELECT COUNT(*) FROM accounts WHERE status = 'disabled'")?,
            vaults: scalar("SELECT COUNT(*) FROM vaults")?,
            devices: scalar("SELECT COUNT(*) FROM server_devices WHERE revoked = 0")?,
            events: scalar("SELECT COUNT(*) FROM server_events")?,
            storage_bytes: scalar("SELECT COALESCE(SUM(LENGTH(payload)), 0) FROM server_events")?,
        })
    }
}

/// Format the current instant as an RFC 3339 timestamp.
fn rfc3339_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// SRP registration material + identity for a new self-hosted account.
///
/// All fields are client-supplied and opaque to the server, which stores them
/// verbatim so new devices and issue #130's login flow can re-derive.
pub struct NewAccount<'a> {
    /// Login identifier (an email or a bare username).
    pub username: &'a str,
    /// Random SRP salt (`salt_srp`).
    pub srp_salt: &'a [u8],
    /// SRP verifier `v = g^x mod N` (big-endian), from `tock_crypto::srp`.
    pub srp_verifier: &'a [u8],
    /// SRP group/hash identifier (e.g. `RFC5054-4096-SHA256`).
    pub srp_group: &'a str,
    /// Opaque KDF parameters (JSON) the client needs to re-derive the URK.
    pub kdf_params: &'a str,
    /// Optional invite token (required under invite-only / disabled policies).
    pub invite_token: Option<&'a str>,
}

/// Result of a successful [`ServerDb::register_account`].
pub struct RegisterOutcome {
    /// Server-assigned account id (`UUIDv7`).
    pub account_id: String,
    /// Granted role (`admin` or `user`).
    pub role: String,
    /// Account status (`active`).
    pub status: String,
    /// Interim admin bearer token, present only when the account is an admin.
    pub admin_token: Option<String>,
}

/// A non-secret account row as listed by admin endpoints / CLI.
pub struct UserRecord {
    /// Account id.
    pub id: String,
    /// Login identifier.
    pub username: String,
    /// Role (`admin` or `user`).
    pub role: String,
    /// Status (`active` or `disabled`).
    pub status: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Stored SRP verifier material for an account — the seam consumed by issue
/// #130's login handshake. None of these fields are secret.
pub struct SrpCredentials {
    /// Account id bound into the SRP proof.
    pub account_id: String,
    /// Random SRP salt (`salt_srp`).
    pub srp_salt: Vec<u8>,
    /// SRP verifier `v = g^x mod N` (big-endian).
    pub srp_verifier: Vec<u8>,
    /// SRP group/hash identifier.
    pub srp_group: String,
    /// Opaque KDF parameters the client stored at registration. Returned at
    /// `srp/start` so a fresh device can re-derive the URK before login
    /// (issue #129); never interpreted by the server.
    pub kdf_params: String,
}

/// A live authenticated-sync session resolved from a bearer token hash
/// (issue #130). None of these fields are secret.
pub struct SessionRecord {
    /// Account that owns the session.
    pub account_id: String,
    /// Channel-binding tag mixed into event AAD (defense-in-depth, ADR-010).
    pub channel_binding: Vec<u8>,
    /// Account role (`admin` or `user`) at session-lookup time.
    pub role: String,
}

/// A live authenticated-sync session listed for its owning account (issue
/// #131 self-service). Non-secret: `token_hash` is already the SHA-256 of the
/// bearer token.
pub struct SessionInfo {
    /// SHA-256 hash of the session bearer token (its stable identifier).
    pub token_hash: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// Absolute expiry (Unix seconds).
    pub expires_at: i64,
}

/// A device registered under one of an account's vaults (issue #131
/// self-service). Non-secret metadata only.
pub struct DeviceInfo {
    /// Raw 16-byte device id.
    pub device_id: Vec<u8>,
    /// Optional human label supplied at registration.
    pub label: Option<String>,
    /// RFC 3339 registration timestamp.
    pub registered_at: String,
    /// Whether the device has been revoked.
    pub revoked: bool,
}

/// Aggregate instance statistics for the admin usage/health panel (issue
/// #131). Counts and byte totals over opaque, encrypted data.
pub struct InstanceStats {
    /// Total accounts.
    pub accounts_total: i64,
    /// Accounts with the `admin` role.
    pub accounts_admin: i64,
    /// Active accounts.
    pub accounts_active: i64,
    /// Disabled accounts.
    pub accounts_disabled: i64,
    /// Total vaults.
    pub vaults: i64,
    /// Non-revoked registered devices.
    pub devices: i64,
    /// Total stored events.
    pub events: i64,
    /// Total bytes of stored (encrypted) event payloads.
    pub storage_bytes: i64,
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
    fn all_events_orders_by_rowid() {
        let db = ServerDb::open_memory().expect("open");
        let vault = [1_u8; 16];
        let other = [9_u8; 16];
        let device = [2_u8; 16];

        db.ensure_vault(&vault).expect("vault");
        db.ensure_vault(&other).expect("other vault");
        for i in 1..=3 {
            let mut eid = [0_u8; 16];
            eid[0] = i;
            db.push_event(&eid, &vault, &device, i64::from(i), &[i])
                .expect("push");
        }
        // An event in a different vault must not appear in this vault's export.
        db.push_event(&[0xFF; 16], &other, &device, 1, b"other")
            .expect("push other");

        let all = db.all_events(&vault).expect("all");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].lamport, 1);
        assert_eq!(all[2].lamport, 3);
        // Monotonic rowid ordering.
        assert!(all[0].rowid < all[1].rowid && all[1].rowid < all[2].rowid);
    }

    #[test]
    fn list_and_scope_vaults() {
        let db = ServerDb::open_memory().expect("open");
        let owned = [1_u8; 16];
        let unowned = [2_u8; 16];
        db.ensure_vault(&owned).expect("owned");
        db.ensure_vault(&unowned).expect("unowned");
        db.claim_vault_for_account(&owned, "acct-1").expect("claim");

        let vaults = db.list_vaults().expect("list");
        assert_eq!(vaults.len(), 2);
        assert!(
            vaults
                .iter()
                .any(|(id, acct)| id == &owned.to_vec() && acct.as_deref() == Some("acct-1"))
        );
        assert!(
            vaults
                .iter()
                .any(|(id, acct)| id == &unowned.to_vec() && acct.is_none())
        );

        let scoped = db.vaults_for_account("acct-1").expect("scoped");
        assert_eq!(scoped, vec![owned]);
        assert!(db.vaults_for_account("acct-2").expect("none").is_empty());
    }

    #[test]
    fn snapshot_to_produces_openable_copy() {
        let tmp = tempfile::tempdir().expect("tmp");
        let src_path = tmp.path().join("tock-server.db");
        let db = ServerDb::open(&src_path).expect("open");
        let vault = [7_u8; 16];
        let device = [8_u8; 16];
        db.ensure_vault(&vault).expect("vault");
        db.push_event(&[1_u8; 16], &vault, &device, 1, b"opaque")
            .expect("push");

        let snap_path = tmp.path().join("snapshot.db");
        db.snapshot_to(&snap_path).expect("snapshot");
        assert!(snap_path.exists());

        // The snapshot is a real, consistent database carrying the same event.
        let restored = ServerDb::open(&snap_path).expect("open snapshot");
        let events = restored.all_events(&vault).expect("all");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload, b"opaque");
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
            Err(Error::Forbidden(_))
        ));
        assert!(matches!(
            db.claim_vault_for_account(&vault, &aid2),
            Err(Error::Forbidden(_))
        ));
    }

    // ── Self-hosted account system (issue #127) ──────────────────────

    use crate::accounts::RegistrationPolicy;

    fn sample_account<'a>(username: &'a str, invite: Option<&'a str>) -> NewAccount<'a> {
        NewAccount {
            username,
            srp_salt: &[0xAA; 16],
            srp_verifier: &[0xBB; 64],
            srp_group: "RFC5054-4096-SHA256",
            kdf_params: "argon2id$t=3$m=65536$p=1",
            invite_token: invite,
        }
    }

    #[test]
    fn first_registration_bootstraps_admin() {
        let db = ServerDb::open_memory().expect("open");
        assert_eq!(db.account_count().expect("count"), 0);

        let out = db
            .register_account(&sample_account("alice", None), RegistrationPolicy::Disabled)
            .expect("register");

        assert_eq!(out.role, "admin");
        assert_eq!(out.status, "active");
        let token = out.admin_token.expect("admin token minted");
        assert!(token.starts_with("adm_"));
        assert_eq!(db.account_count().expect("count"), 1);

        // The minted token resolves back to the admin account.
        let resolved = db
            .admin_account_id_by_token(&token)
            .expect("lookup")
            .expect("present");
        assert_eq!(resolved, out.account_id);
    }

    #[test]
    fn disabled_policy_requires_invite_after_bootstrap() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(&sample_account("admin", None), RegistrationPolicy::Disabled)
            .expect("bootstrap");

        // No invite → forbidden.
        assert!(matches!(
            db.register_account(&sample_account("bob", None), RegistrationPolicy::Disabled),
            Err(Error::Forbidden(_))
        ));

        // With a valid invite → ordinary user.
        let invite = db.create_invite(None, "user").expect("invite");
        let out = db
            .register_account(
                &sample_account("bob", Some(&invite)),
                RegistrationPolicy::Disabled,
            )
            .expect("register with invite");
        assert_eq!(out.role, "user");
        assert!(out.admin_token.is_none());
    }

    #[test]
    fn open_policy_allows_registration_without_invite() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(&sample_account("admin", None), RegistrationPolicy::Open)
            .expect("bootstrap");

        let out = db
            .register_account(&sample_account("carol", None), RegistrationPolicy::Open)
            .expect("open register");
        assert_eq!(out.role, "user");
    }

    #[test]
    fn invite_only_policy_requires_invite() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(
            &sample_account("admin", None),
            RegistrationPolicy::InviteOnly,
        )
        .expect("bootstrap");

        assert!(matches!(
            db.register_account(
                &sample_account("dave", None),
                RegistrationPolicy::InviteOnly
            ),
            Err(Error::Forbidden(_))
        ));
    }

    #[test]
    fn invite_is_single_use() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(&sample_account("admin", None), RegistrationPolicy::Disabled)
            .expect("bootstrap");
        let invite = db.create_invite(None, "user").expect("invite");

        db.register_account(
            &sample_account("first", Some(&invite)),
            RegistrationPolicy::Disabled,
        )
        .expect("first use");

        assert!(matches!(
            db.register_account(
                &sample_account("second", Some(&invite)),
                RegistrationPolicy::Disabled,
            ),
            Err(Error::Forbidden(_))
        ));
    }

    #[test]
    fn invite_can_grant_admin_role() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(&sample_account("admin", None), RegistrationPolicy::Disabled)
            .expect("bootstrap");
        let invite = db.create_invite(Some("eve"), "admin").expect("invite");

        // Pinned to "eve": a different username is rejected.
        assert!(matches!(
            db.register_account(
                &sample_account("mallory", Some(&invite)),
                RegistrationPolicy::Disabled,
            ),
            Err(Error::Forbidden(_))
        ));

        let out = db
            .register_account(
                &sample_account("eve", Some(&invite)),
                RegistrationPolicy::Disabled,
            )
            .expect("admin invite register");
        assert_eq!(out.role, "admin");
        assert!(out.admin_token.is_some());
    }

    #[test]
    fn duplicate_username_conflicts() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(&sample_account("admin", None), RegistrationPolicy::Open)
            .expect("bootstrap");
        db.register_account(&sample_account("frank", None), RegistrationPolicy::Open)
            .expect("first");

        assert!(matches!(
            db.register_account(&sample_account("frank", None), RegistrationPolicy::Open),
            Err(Error::Conflict(_))
        ));
    }

    // ── Identifier normalization (issue #199, ADR-016 §5) ────────────────

    #[test]
    fn register_rejects_case_and_unicode_near_duplicates() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(&sample_account("admin", None), RegistrationPolicy::Open)
            .expect("bootstrap");
        db.register_account(&sample_account("alice", None), RegistrationPolicy::Open)
            .expect("first alice");

        // Every case/Unicode-equivalent spelling collapses to the same stored
        // identifier and is refused as a duplicate.
        let fullwidth = "\u{FF41}\u{FF4C}\u{FF49}\u{FF43}\u{FF45}"; // "ａｌｉｃｅ"
        let combining = "alice\u{0301}"; // "alicé" via combining acute — differs from "alice"
        for near in ["Alice", "ALICE", "  alice  ", fullwidth] {
            assert!(
                matches!(
                    db.register_account(&sample_account(near, None), RegistrationPolicy::Open),
                    Err(Error::Conflict(_))
                ),
                "near-duplicate {near:?} should conflict with 'alice'"
            );
        }

        // A genuinely different identifier (an extra combining mark, or a plain
        // different name) is allowed.
        db.register_account(&sample_account(combining, None), RegistrationPolicy::Open)
            .expect("distinct combining variant");
        db.register_account(&sample_account("bob", None), RegistrationPolicy::Open)
            .expect("distinct name");
    }

    #[test]
    fn register_stores_normalized_form() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(&sample_account("admin", None), RegistrationPolicy::Open)
            .expect("bootstrap");
        db.register_account(
            &sample_account("Alice@Example.COM", None),
            RegistrationPolicy::Open,
        )
        .expect("register mixed-case");

        // The row is persisted under the normalized (lowercased) identifier.
        let creds = db
            .get_srp_credentials("alice@example.com")
            .expect("lookup")
            .expect("found normalized");
        assert!(!creds.account_id.is_empty());
    }

    #[test]
    fn srp_lookup_is_case_and_unicode_insensitive() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(&sample_account("admin", None), RegistrationPolicy::Open)
            .expect("bootstrap");
        db.register_account(&sample_account("alice", None), RegistrationPolicy::Open)
            .expect("register alice");

        let fullwidth = "\u{FF21}\u{FF2C}\u{FF29}\u{FF23}\u{FF25}"; // "ＡＬＩＣＥ"
        for spelling in ["alice", "Alice", "ALICE", "  alice ", fullwidth] {
            assert!(
                db.get_srp_credentials(spelling).expect("lookup").is_some(),
                "login lookup for {spelling:?} should resolve to the registered account"
            );
        }
    }

    // ── Adoption: atomic register + claim + header (issue #197, ADR-016 §5) ──

    #[test]
    fn adopt_register_claims_vault_and_stores_header_atomically() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(&sample_account("admin", None), RegistrationPolicy::Open)
            .expect("bootstrap");

        let vault = [0x11_u8; 16];
        let header = b"wrapped-header-bytes".to_vec();
        let out = db
            .adopt_register(
                &sample_account("alice", None),
                RegistrationPolicy::Open,
                Some((&vault, &header)),
            )
            .expect("adopt");

        // Account exists, vault is claimed for it, and the header is stored —
        // all committed together.
        db.require_vault_access(&vault, &out.account_id)
            .expect("claimed");
        assert_eq!(
            db.get_vault_header(&vault).expect("header").as_deref(),
            Some(header.as_slice())
        );
    }

    #[test]
    fn adopt_register_duplicate_username_consumes_nothing() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(&sample_account("admin", None), RegistrationPolicy::Open)
            .expect("bootstrap");

        let vault_a = [0x11_u8; 16];
        let header_a = b"header-a".to_vec();
        db.adopt_register(
            &sample_account("alice", None),
            RegistrationPolicy::Open,
            Some((&vault_a, &header_a)),
        )
        .expect("first adopt");
        let count_before = db.account_count().expect("count");

        // A second adopt reusing the SAME username but a DIFFERENT vault must
        // fail — and roll back entirely: no new account, no claim on vault_b,
        // no header stored, and the first vault's header is untouched.
        let vault_b = [0x22_u8; 16];
        let header_b = b"header-b".to_vec();
        assert!(matches!(
            db.adopt_register(
                &sample_account("alice", None),
                RegistrationPolicy::Open,
                Some((&vault_b, &header_b)),
            ),
            Err(Error::Conflict(_))
        ));

        assert_eq!(db.account_count().expect("count"), count_before);
        assert!(
            db.get_vault_header(&vault_b).expect("header b").is_none(),
            "failed adopt must store no header"
        );
        assert!(
            matches!(
                db.require_vault_access(&vault_b, "whoever"),
                Err(Error::NotFound)
            ),
            "failed adopt must not create the vault bucket"
        );
        assert_eq!(
            db.get_vault_header(&vault_a).expect("header a").as_deref(),
            Some(header_a.as_slice()),
            "prior state stays intact (retry-safe)"
        );
    }

    #[test]
    fn disconnect_account_revokes_principal_but_preserves_data() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(&sample_account("admin", None), RegistrationPolicy::Open)
            .expect("bootstrap");

        let vault = [0x33_u8; 16];
        let header = b"still-here".to_vec();
        let out = db
            .adopt_register(
                &sample_account("alice", None),
                RegistrationPolicy::Open,
                Some((&vault, &header)),
            )
            .expect("adopt");

        assert!(db.disconnect_account(&out.account_id).expect("disconnect"));

        // The principal (B) is gone: the account no longer exists and the vault
        // is disowned (re-adoptable). The wrapped header — opaque data — stays.
        assert!(db.get_account(&out.account_id).expect("get").is_none());
        assert!(matches!(
            db.require_vault_access(&vault, &out.account_id),
            Err(Error::Forbidden(_))
        ));
        assert_eq!(
            db.get_vault_header(&vault).expect("header").as_deref(),
            Some(header.as_slice())
        );

        // Idempotent: disconnecting an unknown principal is a no-op false.
        assert!(!db.disconnect_account("no-such-account").expect("noop"));
    }

    #[test]
    fn disable_enable_and_delete_user() {
        let db = ServerDb::open_memory().expect("open");
        db.register_account(&sample_account("admin", None), RegistrationPolicy::Open)
            .expect("bootstrap");
        let user = db
            .register_account(&sample_account("grace", None), RegistrationPolicy::Open)
            .expect("user");

        assert!(
            db.set_user_status(&user.account_id, "disabled")
                .expect("disable")
        );
        let listed = db.list_users().expect("list");
        let row = listed
            .iter()
            .find(|u| u.id == user.account_id)
            .expect("present");
        assert_eq!(row.status, "disabled");

        assert!(
            db.set_user_status(&user.account_id, "active")
                .expect("enable")
        );
        assert!(db.delete_user(&user.account_id).expect("delete"));
        assert!(!db.delete_user(&user.account_id).expect("delete again"));
    }

    #[test]
    fn disabling_admin_revokes_admin_token() {
        let db = ServerDb::open_memory().expect("open");
        let out = db
            .register_account(&sample_account("admin", None), RegistrationPolicy::Open)
            .expect("bootstrap");
        let token = out.admin_token.expect("token");

        assert!(
            db.admin_account_id_by_token(&token)
                .expect("lookup")
                .is_some()
        );

        db.set_user_status(&out.account_id, "disabled")
            .expect("disable");
        assert!(
            db.admin_account_id_by_token(&token)
                .expect("lookup")
                .is_none()
        );
    }

    #[test]
    fn srp_credentials_seam_excludes_billing_and_disabled() {
        let db = ServerDb::open_memory().expect("open");
        let out = db
            .register_account(&sample_account("heidi", None), RegistrationPolicy::Open)
            .expect("register");

        let creds = db
            .get_srp_credentials("heidi")
            .expect("query")
            .expect("present");
        assert_eq!(creds.account_id, out.account_id);
        assert_eq!(creds.srp_salt, vec![0xAA; 16]);
        assert_eq!(creds.srp_verifier, vec![0xBB; 64]);
        assert_eq!(creds.srp_group, "RFC5054-4096-SHA256");

        // Hosted billing rows have no SRP material.
        db.create_account("billing@example.com", crate::billing::Tier::Personal)
            .expect("billing");
        assert!(
            db.get_srp_credentials("billing@example.com")
                .expect("query")
                .is_none()
        );

        // Disabled accounts are excluded from the login seam.
        db.set_user_status(&out.account_id, "disabled")
            .expect("disable");
        assert!(db.get_srp_credentials("heidi").expect("query").is_none());
    }

    #[test]
    fn session_lifecycle_lookup_expiry_and_refresh() {
        let db = ServerDb::open_memory().expect("open");
        let out = db
            .register_account(&sample_account("ivan", None), RegistrationPolicy::Open)
            .expect("register");
        let now = 1_000_i64;
        let cb = vec![0x11_u8; 32];

        // Live session resolves; an unknown hash does not.
        db.create_session("hash-a", &out.account_id, &cb, now + 100)
            .expect("create");
        let rec = db
            .lookup_session("hash-a", now)
            .expect("lookup")
            .expect("present");
        assert_eq!(rec.account_id, out.account_id);
        assert_eq!(rec.channel_binding, cb);
        assert_eq!(rec.role, "admin"); // first account bootstraps as admin
        assert!(db.lookup_session("missing", now).expect("lookup").is_none());

        // Expired sessions are not returned.
        db.create_session("hash-b", &out.account_id, &cb, now - 1)
            .expect("create expired");
        assert!(db.lookup_session("hash-b", now).expect("lookup").is_none());

        // Refresh slides a live session forward but refuses an expired one.
        assert!(
            db.refresh_session("hash-a", now, now + 500)
                .expect("refresh")
        );
        assert!(
            db.lookup_session("hash-a", now + 400)
                .expect("lookup")
                .is_some()
        );
        assert!(
            !db.refresh_session("hash-b", now, now + 500)
                .expect("refresh")
        );

        // Disabling the account revokes the session even before expiry.
        db.set_user_status(&out.account_id, "disabled")
            .expect("disable");
        assert!(db.lookup_session("hash-a", now).expect("lookup").is_none());

        // Housekeeping prunes only past-due rows.
        db.set_user_status(&out.account_id, "active")
            .expect("enable");
        let removed = db.delete_expired_sessions(now).expect("prune");
        assert_eq!(removed, 1); // hash-b only
        assert!(db.lookup_session("hash-a", now).expect("lookup").is_some());
    }

    #[test]
    fn registration_policy_defaults_to_disabled_and_persists() {
        let db = ServerDb::open_memory().expect("open");
        assert_eq!(
            db.registration_policy().expect("policy"),
            RegistrationPolicy::Disabled
        );

        db.set_registration_policy(RegistrationPolicy::Open)
            .expect("set");
        assert_eq!(
            db.registration_policy().expect("policy"),
            RegistrationPolicy::Open
        );

        // Upsert (not duplicate insert) on second write.
        db.set_registration_policy(RegistrationPolicy::InviteOnly)
            .expect("set 2");
        assert_eq!(
            db.registration_policy().expect("policy"),
            RegistrationPolicy::InviteOnly
        );
    }

    #[test]
    fn migration_preserves_legacy_billing_account() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("legacy.db");

        // Build a pre-#127 database: v1 schema + a legacy billing row.
        {
            let conn = Connection::open(&path).expect("open raw");
            conn.execute_batch(MIGRATIONS[0].sql).expect("v1 schema");
            conn.execute_batch("PRAGMA user_version = 1;")
                .expect("stamp v1");
            conn.execute(
                "INSERT INTO accounts (id, email, api_token, tier, created_at)
                 VALUES ('acct-legacy', 'legacy@example.com', 'tok_legacy', 'personal', '2024-01-01T00:00:00Z')",
                [],
            )
            .expect("insert legacy");
        }

        // Opening through ServerDb runs the v2 migration.
        let db = ServerDb::open(&path).expect("open + migrate");

        let users = db.list_users().expect("list");
        let row = users.iter().find(|u| u.id == "acct-legacy").expect("kept");
        assert_eq!(row.username, "legacy@example.com");
        assert_eq!(row.role, "user");
        assert_eq!(row.status, "active");

        // Hosted billing lookups still resolve via the preserved token.
        let resolved = db
            .account_id_by_api_token("tok_legacy")
            .expect("lookup")
            .expect("present");
        assert_eq!(resolved, "acct-legacy");

        // New v2 tables exist and behave.
        assert_eq!(
            db.registration_policy().expect("policy"),
            RegistrationPolicy::Disabled
        );
    }
}
