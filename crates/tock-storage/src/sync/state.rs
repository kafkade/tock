//! Persistence helpers for the sync bookkeeping tables defined in
//! migration `0011_sync_state.sql`: the `sync_state` key/value bag, the
//! `sync_journal`, `entity_heads`, and `sync_conflicts`.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use tock_core::event::{DeviceId, EntityKind, VectorClock};

use crate::Error;

// ── sync_state key/value bag ─────────────────────────────────────────

/// Read a raw `sync_state` value.
pub fn get_state(conn: &Connection, key: &str) -> Result<Option<Vec<u8>>, Error> {
    let value: Option<Vec<u8>> = conn
        .query_row(
            "SELECT value FROM sync_state WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value)
}

/// Write a raw `sync_state` value.
pub fn set_state(conn: &Connection, key: &str, value: &[u8]) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO sync_state (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Read a UTF-8 `sync_state` value.
pub fn get_state_str(conn: &Connection, key: &str) -> Result<Option<String>, Error> {
    match get_state(conn, key)? {
        Some(bytes) => Ok(Some(
            String::from_utf8(bytes).map_err(|_| Error::EventLogIntegrity)?,
        )),
        None => Ok(None),
    }
}

/// Write a UTF-8 `sync_state` value.
pub fn set_state_str(conn: &Connection, key: &str, value: &str) -> Result<(), Error> {
    set_state(conn, key, value.as_bytes())
}

/// Read a `u64` cursor from `sync_state` (defaults to 0).
pub fn get_cursor(conn: &Connection, key: &str) -> Result<u64, Error> {
    match get_state(conn, key)? {
        Some(bytes) => {
            let arr: [u8; 8] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| Error::EventLogIntegrity)?;
            Ok(u64::from_be_bytes(arr))
        }
        None => Ok(0),
    }
}

/// Write a `u64` cursor to `sync_state`.
pub fn set_cursor(conn: &Connection, key: &str, value: u64) -> Result<(), Error> {
    set_state(conn, key, &value.to_be_bytes())
}

// ── Vector clock persistence ─────────────────────────────────────────

const VECTOR_CLOCK_KEY: &str = "vector_clock";

/// Load the local vector clock (empty if unset).
pub fn load_vector_clock(conn: &Connection) -> Result<VectorClock, Error> {
    get_state(conn, VECTOR_CLOCK_KEY)?.map_or_else(
        || Ok(VectorClock::new()),
        |bytes| decode_vector_clock(&bytes),
    )
}

/// Persist the local vector clock.
pub fn store_vector_clock(conn: &Connection, vc: &VectorClock) -> Result<(), Error> {
    set_state(conn, VECTOR_CLOCK_KEY, &encode_vector_clock(vc))
}

fn encode_vector_clock(vc: &VectorClock) -> Vec<u8> {
    let mut out = Vec::new();
    let count = u32::try_from(vc.0.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&count.to_be_bytes());
    for (dev, lamport) in &vc.0 {
        out.extend_from_slice(dev.as_bytes());
        out.extend_from_slice(&lamport.to_be_bytes());
    }
    out
}

fn decode_vector_clock(bytes: &[u8]) -> Result<VectorClock, Error> {
    if bytes.len() < 4 {
        return Err(Error::EventLogIntegrity);
    }
    let count_arr: [u8; 4] = bytes[0..4]
        .try_into()
        .map_err(|_| Error::EventLogIntegrity)?;
    let count = u32::from_be_bytes(count_arr) as usize;
    let mut map = BTreeMap::new();
    let mut off = 4;
    for _ in 0..count {
        let end = off + 24;
        if end > bytes.len() {
            return Err(Error::EventLogIntegrity);
        }
        let dev: [u8; 16] = bytes[off..off + 16]
            .try_into()
            .map_err(|_| Error::EventLogIntegrity)?;
        let lam: [u8; 8] = bytes[off + 16..end]
            .try_into()
            .map_err(|_| Error::EventLogIntegrity)?;
        map.insert(DeviceId::from_bytes(dev), u64::from_be_bytes(lam));
        off = end;
    }
    Ok(VectorClock(map))
}

// ── sync_journal ─────────────────────────────────────────────────────

/// A journal entry: the last-emitted canonical content for an entity.
pub struct JournalEntry {
    /// Canonical row snapshot frame at last emit.
    pub content: Vec<u8>,
}

/// Load the whole journal for an entity kind, keyed by sync id.
pub fn load_journal(
    conn: &Connection,
    kind: EntityKind,
) -> Result<BTreeMap<Uuid, JournalEntry>, Error> {
    let mut stmt =
        conn.prepare("SELECT entity_id, content FROM sync_journal WHERE entity_kind = ?1")?;
    let mut rows = stmt.query(params![kind.as_str()])?;
    let mut out = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let id_bytes: Vec<u8> = row.get(0)?;
        let content: Vec<u8> = row.get(1)?;
        let arr: [u8; 16] = id_bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::EventLogIntegrity)?;
        out.insert(Uuid::from_bytes(arr), JournalEntry { content });
    }
    Ok(out)
}

/// Upsert one journal entry.
pub fn put_journal(
    conn: &Connection,
    kind: EntityKind,
    entity_id: Uuid,
    content: &[u8],
    lamport: u64,
) -> Result<(), Error> {
    let lamport_i = i64::try_from(lamport).unwrap_or(i64::MAX);
    conn.execute(
        "INSERT INTO sync_journal (entity_kind, entity_id, content, lamport)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(entity_kind, entity_id) DO UPDATE SET
           content = excluded.content, lamport = excluded.lamport",
        params![
            kind.as_str(),
            entity_id.as_bytes().to_vec(),
            content,
            lamport_i
        ],
    )?;
    Ok(())
}

/// Delete one journal entry.
pub fn delete_journal(conn: &Connection, kind: EntityKind, entity_id: Uuid) -> Result<(), Error> {
    conn.execute(
        "DELETE FROM sync_journal WHERE entity_kind = ?1 AND entity_id = ?2",
        params![kind.as_str(), entity_id.as_bytes().to_vec()],
    )?;
    Ok(())
}

// ── entity_heads ─────────────────────────────────────────────────────

/// Load the head event ids for an entity.
pub fn load_heads(
    conn: &Connection,
    kind: EntityKind,
    entity_id: Uuid,
) -> Result<Vec<Uuid>, Error> {
    let mut stmt = conn.prepare(
        "SELECT head_event_id FROM entity_heads
         WHERE entity_kind = ?1 AND entity_id = ?2",
    )?;
    let mut rows = stmt.query(params![kind.as_str(), entity_id.as_bytes().to_vec()])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let bytes: Vec<u8> = row.get(0)?;
        let arr: [u8; 16] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::EventLogIntegrity)?;
        out.push(Uuid::from_bytes(arr));
    }
    Ok(out)
}

/// Replace the head set for an entity.
pub fn set_heads(
    conn: &Connection,
    kind: EntityKind,
    entity_id: Uuid,
    heads: &[Uuid],
) -> Result<(), Error> {
    conn.execute(
        "DELETE FROM entity_heads WHERE entity_kind = ?1 AND entity_id = ?2",
        params![kind.as_str(), entity_id.as_bytes().to_vec()],
    )?;
    for head in heads {
        conn.execute(
            "INSERT OR IGNORE INTO entity_heads (entity_kind, entity_id, head_event_id)
             VALUES (?1, ?2, ?3)",
            params![
                kind.as_str(),
                entity_id.as_bytes().to_vec(),
                head.as_bytes().to_vec()
            ],
        )?;
    }
    Ok(())
}

// ── sync_conflicts ───────────────────────────────────────────────────

/// A surfaced, unresolved conflict.
pub struct SyncConflict {
    /// Conflict record id.
    pub id: Uuid,
    /// Entity kind involved.
    pub entity_kind: String,
    /// Entity sync id involved.
    pub entity_id: Uuid,
    /// Human-readable detail.
    pub detail: String,
    /// When the conflict was recorded (RFC-3339).
    pub created_at: String,
}

/// Record a conflict for later review.
pub fn record_conflict(
    conn: &Connection,
    kind: EntityKind,
    entity_id: Uuid,
    detail: &str,
) -> Result<(), Error> {
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|_| Error::EventLogIntegrity)?;
    conn.execute(
        "INSERT INTO sync_conflicts (id, entity_kind, entity_id, detail, created_at, resolved)
         VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        params![
            Uuid::now_v7().as_bytes().to_vec(),
            kind.as_str(),
            entity_id.as_bytes().to_vec(),
            detail,
            now,
        ],
    )?;
    Ok(())
}

/// List unresolved conflicts (newest first).
///
/// # Errors
/// [`Error::Sqlite`] on query failure, or [`Error::EventLogIntegrity`]
/// if a stored id column is not a 16-byte value.
pub fn list_conflicts(conn: &Connection) -> Result<Vec<SyncConflict>, Error> {
    let mut stmt = conn.prepare(
        "SELECT id, entity_kind, entity_id, detail, created_at
         FROM sync_conflicts WHERE resolved = 0
         ORDER BY created_at DESC",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let id_bytes: Vec<u8> = row.get(0)?;
        let entity_bytes: Vec<u8> = row.get(2)?;
        let id_arr: [u8; 16] = id_bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::EventLogIntegrity)?;
        let entity_arr: [u8; 16] = entity_bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::EventLogIntegrity)?;
        out.push(SyncConflict {
            id: Uuid::from_bytes(id_arr),
            entity_kind: row.get(1)?,
            entity_id: Uuid::from_bytes(entity_arr),
            detail: row.get(3)?,
            created_at: row.get(4)?,
        });
    }
    Ok(out)
}

/// Mark a conflict resolved. Returns whether a matching row was updated.
///
/// # Errors
/// [`Error::Sqlite`] on update failure.
pub fn resolve_conflict(conn: &Connection, id: Uuid) -> Result<bool, Error> {
    let n = conn.execute(
        "UPDATE sync_conflicts SET resolved = 1 WHERE id = ?1",
        params![id.as_bytes().to_vec()],
    )?;
    Ok(n > 0)
}
