//! Embedded SQL migrations and the runner that applies them.
//!
//! Migrations are numbered, embedded via `include_str!`, and stored in
//! `schema_migrations` with a sha-256 checksum of the migration SQL.
//! On open, the runner re-hashes each already-applied migration's SQL
//! and refuses to start if any differ — this is a **developer/schema
//! consistency** check, not a security control (an attacker with
//! binary modification capability can change both the SQL and the
//! checksum verification logic).

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::Error;

/// One embedded migration.
pub struct Migration {
    /// Monotonic version (matches `PRAGMA user_version` after apply).
    pub version: u32,
    /// Human-readable name (also stored for diagnostics).
    pub name: &'static str,
    /// The SQL text to execute.
    pub sql: &'static str,
}

/// All migrations bundled with this build, in apply order.
pub const ALL: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial",
        sql: include_str!("migrations/0001_initial.sql"),
    },
    Migration {
        version: 2,
        name: "event_log",
        sql: include_str!("migrations/0002_event_log.sql"),
    },
    Migration {
        version: 3,
        name: "domain_tables",
        sql: include_str!("migrations/0003_domain_tables.sql"),
    },
    Migration {
        version: 4,
        name: "time_blocks",
        sql: include_str!("migrations/0004_time_blocks.sql"),
    },
    Migration {
        version: 5,
        name: "focus_sessions",
        sql: include_str!("migrations/0005_focus_sessions.sql"),
    },
    Migration {
        version: 6,
        name: "habits",
        sql: include_str!("migrations/0006_habits.sql"),
    },
    Migration {
        version: 7,
        name: "uda_definitions",
        sql: include_str!("migrations/0007_uda_definitions.sql"),
    },
    Migration {
        version: 8,
        name: "saved_reports",
        sql: include_str!("migrations/0008_saved_reports.sql"),
    },
    Migration {
        version: 9,
        name: "contexts",
        sql: include_str!("migrations/0009_contexts.sql"),
    },
];

/// Compute the sha-256 hex digest of `sql`.
#[must_use]
pub fn checksum(sql: &str) -> String {
    use std::fmt::Write as _;
    let mut h = Sha256::new();
    h.update(sql.as_bytes());
    let out = h.finalize();
    let mut s = String::with_capacity(out.len() * 2);
    for b in out {
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

/// Apply any missing migrations to `conn`.
///
/// The first migration is responsible for creating the
/// `schema_migrations` table itself, so the runner queries
/// `PRAGMA user_version` first (which is `0` on a fresh database) to
/// decide whether the table exists.
///
/// # Errors
/// - [`Error::Sqlite`] for any SQL failure.
/// - [`Error::MigrationChecksumMismatch`] if a recorded checksum no
///   longer matches the embedded SQL.
pub fn migrate(conn: &mut Connection) -> Result<u32, Error> {
    let current: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);

    // Verify checksums of already-applied migrations (only after the
    // schema_migrations table exists, i.e. after migration 1).
    if current >= 1 {
        for m in ALL.iter().take_while(|m| m.version <= current) {
            let stored: Option<String> = conn
                .query_row(
                    "SELECT checksum FROM schema_migrations WHERE version = ?1",
                    params![m.version],
                    |r| r.get(0),
                )
                .ok();
            if let Some(stored) = stored
                && stored != checksum(m.sql)
            {
                return Err(Error::MigrationChecksumMismatch { version: m.version });
            }
        }
    }

    for m in ALL {
        if m.version <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)?;
        let ts = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default();
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at, checksum)
             VALUES (?1, ?2, ?3, ?4)",
            params![m.version, m.name, ts, checksum(m.sql)],
        )?;
        // PRAGMA user_version takes an integer literal, not a bind parameter.
        tx.execute_batch(&format!("PRAGMA user_version = {};", m.version))?;
        tx.commit()?;
    }

    let final_v: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);
    Ok(final_v)
}
