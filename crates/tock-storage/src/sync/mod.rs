//! Sync-time state-diff substrate.
//!
//! Domain repositories write directly to `SQLite` and never emit events.
//! This module bridges that gap at sync time: it synthesizes signed,
//! AEAD-encrypted events by diffing the current domain state against a
//! per-entity journal ([`collect_local_changes`]), and materializes
//! remote events back into the domain tables after running them through
//! the [`tock_sync`] conflict engine ([`ingest_events`]).
//!
//! See `docs/adr/003` (event-sourced sync). All wire payloads are
//! ciphertext; the journal/heads/clock bookkeeping lives in the tables
//! created by migration `0011_sync_state.sql`.

mod registry;
mod row;
mod state;

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use tock_core::event::{DeviceId, EntityKind, EventOp, SignedEvent, VectorClock, verify};
use tock_crypto::signature::VerifyingKey;
use tock_sync::conflict::ConflictPolicy;
use tock_sync::engine::{IngestAction, process_incoming_event};
use uuid::Uuid;

use crate::Error;
use crate::event_log::{self, EventLog};
use crate::vault::OpenVault;

pub use state::{SyncConflict, list_conflicts, resolve_conflict};

// ── Public configuration accessors ───────────────────────────────────

/// `sync_state` key for the configured server base URL.
const KEY_SERVER_URL: &str = "server_url";
/// `sync_state` key for this device's human label.
const KEY_DEVICE_LABEL: &str = "device_label";
/// `sync_state` key for the server pull cursor (opaque monotonic position).
const KEY_PULL_CURSOR: &str = "pull_cursor";

/// Columns excluded from an `Update` event's changed-field list (but
/// still carried in the snapshot payload). These are derived metadata
/// timestamps that change on every edit; including them would make every
/// pair of concurrent edits look field-overlapping and force a
/// last-write-wins resolution instead of a clean field merge.
const MERGE_IGNORE_FIELDS: &[&str] = &["modified_at"];

/// Read the persisted sync server URL.
///
/// # Errors
/// [`Error::Sqlite`] on query failure.
pub fn server_url(vault: &OpenVault) -> Result<Option<String>, Error> {
    state::get_state_str(vault.connection(), KEY_SERVER_URL)
}

/// Persist the sync server URL.
///
/// # Errors
/// [`Error::Sqlite`] on persistence failure.
pub fn set_server_url(vault: &OpenVault, url: &str) -> Result<(), Error> {
    state::set_state_str(vault.connection(), KEY_SERVER_URL, url)
}

/// Read this device's human label.
///
/// # Errors
/// [`Error::Sqlite`] on query failure.
pub fn device_label(vault: &OpenVault) -> Result<Option<String>, Error> {
    state::get_state_str(vault.connection(), KEY_DEVICE_LABEL)
}

/// Persist this device's human label.
///
/// # Errors
/// [`Error::Sqlite`] on persistence failure.
pub fn set_device_label(vault: &OpenVault, label: &str) -> Result<(), Error> {
    state::set_state_str(vault.connection(), KEY_DEVICE_LABEL, label)
}

/// Read the server pull cursor (0 if never synced).
///
/// # Errors
/// [`Error::Sqlite`] on query failure.
pub fn pull_cursor(vault: &OpenVault) -> Result<u64, Error> {
    state::get_cursor(vault.connection(), KEY_PULL_CURSOR)
}

/// Persist the server pull cursor.
///
/// # Errors
/// [`Error::Sqlite`] on persistence failure.
pub fn set_pull_cursor(vault: &OpenVault, cursor: u64) -> Result<(), Error> {
    state::set_cursor(vault.connection(), KEY_PULL_CURSOR, cursor)
}

// ── Outbound: collect local changes ──────────────────────────────────

/// Diff every syncable table against the journal and append a signed
/// event for each create / update / delete.
///
/// Updates the journal, head set, and local vector clock, and returns
/// the newly produced events for the caller to push to the server.
///
/// # Errors
/// - [`Error::EventLogIntegrity`] on payload (de)serialization failure.
/// - [`Error::Crypto`] / [`Error::Sqlite`] on append failure.
pub fn collect_local_changes(vault: &OpenVault) -> Result<Vec<SignedEvent>, Error> {
    let conn = vault.connection();
    let log = EventLog::new(vault);
    let device = DeviceId::from_bytes(vault.local_device().device_id);
    let mut clock = state::load_vector_clock(conn)?;
    let mut running = log.local_lamport_high()?;
    let mut produced = Vec::new();

    for table in registry::TABLES {
        let snaps = row::snapshot_table(conn, table)?;
        let journal = state::load_journal(conn, table.kind)?;
        let mut seen: HashSet<Uuid> = HashSet::new();

        for snap in &snaps {
            seen.insert(snap.sync_id);
            match journal.get(&snap.sync_id) {
                None => {
                    let signed = emit(
                        &log,
                        &mut clock,
                        &mut running,
                        device,
                        table.kind,
                        snap.sync_id,
                        EventOp::Create,
                        &snap.content,
                        None,
                    )?;
                    state::put_journal(
                        conn,
                        table.kind,
                        snap.sync_id,
                        &snap.content,
                        signed.event.lamport,
                    )?;
                    state::set_heads(conn, table.kind, snap.sync_id, &[signed.event.id])?;
                    produced.push(signed);
                }
                Some(entry) if entry.content != snap.content => {
                    let old_cols = row::decode_columns(&entry.content)?;
                    let mut fields = row::changed_fields(&old_cols, &snap.columns);
                    fields.retain(|f| !MERGE_IGNORE_FIELDS.contains(&f.as_str()));
                    let parent = state::load_heads(conn, table.kind, snap.sync_id)?
                        .first()
                        .copied();
                    let signed = emit(
                        &log,
                        &mut clock,
                        &mut running,
                        device,
                        table.kind,
                        snap.sync_id,
                        EventOp::Update { fields },
                        &snap.content,
                        parent,
                    )?;
                    state::put_journal(
                        conn,
                        table.kind,
                        snap.sync_id,
                        &snap.content,
                        signed.event.lamport,
                    )?;
                    state::set_heads(conn, table.kind, snap.sync_id, &[signed.event.id])?;
                    produced.push(signed);
                }
                Some(_) => {}
            }
        }

        for (sync_id, entry) in &journal {
            if seen.contains(sync_id) {
                continue;
            }
            let cols = row::decode_columns(&entry.content)?;
            let key_cols = row::key_columns_of(table, &cols)?;
            let payload = row::encode_columns(&key_cols);
            let parent = state::load_heads(conn, table.kind, *sync_id)?
                .first()
                .copied();
            let signed = emit(
                &log,
                &mut clock,
                &mut running,
                device,
                table.kind,
                *sync_id,
                EventOp::Delete,
                &payload,
                parent,
            )?;
            state::delete_journal(conn, table.kind, *sync_id)?;
            state::set_heads(conn, table.kind, *sync_id, &[signed.event.id])?;
            produced.push(signed);
        }
    }

    state::store_vector_clock(conn, &clock)?;
    Ok(produced)
}

#[allow(clippy::too_many_arguments)]
fn emit(
    log: &EventLog<'_>,
    clock: &mut VectorClock,
    running: &mut u64,
    device: DeviceId,
    kind: EntityKind,
    entity_id: Uuid,
    op: EventOp,
    plaintext: &[u8],
    parent: Option<Uuid>,
) -> Result<SignedEvent, Error> {
    *running += 1;
    clock.0.insert(device, *running);
    log.append(kind, entity_id, op, plaintext, clock.clone(), parent)
}

// ── Inbound: ingest remote events ────────────────────────────────────

/// Outcome of an ingest pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct IngestSummary {
    /// Number of foreign events applied (excludes duplicates already
    /// present locally).
    pub applied: usize,
    /// Number of conflicts recorded for review.
    pub conflicts: usize,
}

/// Verify, decrypt, conflict-resolve, and materialize a batch of remote
/// events into the domain tables.
///
/// Self-events and already-present events are skipped. Updates heads,
/// journal, vector clock, and records any conflicts.
///
/// # Errors
/// - [`Error::EventLogIntegrity`] if an event fails signature or AEAD
///   verification, or its payload cannot be decoded.
/// - [`Error::Sqlite`] on persistence failure.
pub fn ingest_events(vault: &OpenVault, events: &[SignedEvent]) -> Result<IngestSummary, Error> {
    let conn = vault.connection();
    let log = EventLog::new(vault);
    let mut clock = state::load_vector_clock(conn)?;
    let mut summary = IngestSummary::default();

    for signed in events {
        if event_log::event_exists(conn, signed.event.id)? {
            continue;
        }
        register_signer(conn, signed)?;
        verify(signed, |id| lookup_key(conn, id)).map_err(|_| Error::EventLogIntegrity)?;
        let plaintext = log.decrypt_payload(signed)?;

        let kind = signed.event.entity_kind;
        let entity_id = signed.event.entity_id;
        let head_ids = state::load_heads(conn, kind, entity_id)?;
        let mut head_events = Vec::with_capacity(head_ids.len());
        for hid in &head_ids {
            if let Some(ev) = event_log::load_event_meta(conn, *hid)? {
                head_events.push(ev);
            }
        }

        let result = process_incoming_event(
            &signed.event,
            &head_events,
            &clock,
            ConflictPolicy::default(),
        );
        clock = result.merged_clock.clone();

        if !matches!(result.action, IngestAction::Discard) {
            materialize(conn, signed, &plaintext, &result.action)?;
            refresh_journal(conn, signed)?;
        }

        event_log::insert_event(conn, signed)?;
        state::set_heads(conn, kind, entity_id, &result.new_heads)?;

        for conflict in &result.conflicts {
            let detail = format!(
                "winner={} loser={} resolution={:?}",
                conflict.winning_event_id, conflict.losing_event_id, conflict.resolution
            );
            state::record_conflict(conn, kind, entity_id, &detail)?;
            summary.conflicts += 1;
        }
        summary.applied += 1;
    }

    state::store_vector_clock(conn, &clock)?;
    Ok(summary)
}

fn materialize(
    conn: &Connection,
    signed: &SignedEvent,
    plaintext: &[u8],
    action: &IngestAction,
) -> Result<(), Error> {
    let Some(table) = registry::table_for(signed.event.entity_kind) else {
        return Ok(());
    };
    match &signed.event.op {
        EventOp::Delete | EventOp::Purge => {
            let key_cols = row::decode_columns(plaintext)?;
            // Tolerate already-absent rows; record but don't abort.
            if let Err(e) = row::delete_row(conn, table, &key_cols) {
                state::record_conflict(
                    conn,
                    signed.event.entity_kind,
                    signed.event.entity_id,
                    &format!("delete failed: {e}"),
                )?;
            }
        }
        EventOp::Create | EventOp::Update { .. } | EventOp::Append { .. } | EventOp::Snapshot => {
            let incoming = row::decode_columns(plaintext)?;
            let merged = match action {
                IngestAction::MergeFields { .. } => merge_fields(conn, table, signed, &incoming)?,
                _ => incoming,
            };
            if let Err(e) = row::upsert_row(conn, table, &merged) {
                // Constraint violations (e.g. UNIQUE tag name) become a
                // recorded conflict rather than aborting the whole sync.
                state::record_conflict(
                    conn,
                    signed.event.entity_kind,
                    signed.event.entity_id,
                    &format!("materialize failed: {e}"),
                )?;
            }
        }
    }
    Ok(())
}

/// Field-level merge: overlay only the incoming `Update` fields onto the
/// current local row, preserving disjoint local edits. Falls back to the
/// full incoming snapshot if the row is absent locally.
fn merge_fields(
    conn: &Connection,
    table: &registry::SyncTable,
    signed: &SignedEvent,
    incoming: &[(String, rusqlite::types::Value)],
) -> Result<Vec<(String, rusqlite::types::Value)>, Error> {
    let EventOp::Update { fields } = &signed.event.op else {
        return Ok(incoming.to_vec());
    };
    let key_cols = row::key_columns_of(table, incoming)?;
    let Some(mut current) = row::load_row(conn, table, &key_cols)? else {
        return Ok(incoming.to_vec());
    };
    for field in fields {
        if let Some((_, new_val)) = incoming.iter().find(|(n, _)| n == field) {
            if let Some(slot) = current.iter_mut().find(|(n, _)| n == field) {
                slot.1 = new_val.clone();
            } else {
                current.push((field.clone(), new_val.clone()));
            }
        }
    }
    Ok(current)
}

/// After materializing a remote event, rewrite the journal entry from the
/// now-current row so the next outbound collect does not re-emit it.
fn refresh_journal(conn: &Connection, signed: &SignedEvent) -> Result<(), Error> {
    let Some(table) = registry::table_for(signed.event.entity_kind) else {
        return Ok(());
    };
    let kind = signed.event.entity_kind;
    let entity_id = signed.event.entity_id;
    match &signed.event.op {
        EventOp::Delete | EventOp::Purge => {
            state::delete_journal(conn, kind, entity_id)?;
        }
        _ => {
            // Re-snapshot via the registry by scanning for the row whose
            // derived sync id matches. For uuid-keyed tables this is the
            // common path; the scan is over a single entity kind.
            if let Some(content) = current_content(conn, table, entity_id)? {
                state::put_journal(conn, kind, entity_id, &content, signed.event.lamport)?;
            }
        }
    }
    Ok(())
}

/// Find the canonical content frame of the row whose derived sync id is
/// `sync_id`, by scanning the table's current rows.
fn current_content(
    conn: &Connection,
    table: &registry::SyncTable,
    sync_id: Uuid,
) -> Result<Option<Vec<u8>>, Error> {
    for snap in row::snapshot_table(conn, table)? {
        if snap.sync_id == sync_id {
            return Ok(Some(snap.content));
        }
    }
    Ok(None)
}

fn register_signer(conn: &Connection, signed: &SignedEvent) -> Result<(), Error> {
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default();
    conn.execute(
        "INSERT OR IGNORE INTO devices (device_id, verifying_key, label, registered_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            signed.event.device_id.as_bytes().to_vec(),
            signed.signer.to_bytes().to_vec(),
            Option::<String>::None,
            now,
        ],
    )?;
    Ok(())
}

fn lookup_key(conn: &Connection, id: &DeviceId) -> Option<VerifyingKey> {
    let bytes: Option<Vec<u8>> = conn
        .query_row(
            "SELECT verifying_key FROM devices WHERE device_id = ?1",
            params![id.as_bytes().to_vec()],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten();
    let arr: [u8; 32] = bytes?.as_slice().try_into().ok()?;
    VerifyingKey::from_bytes(&arr).ok()
}
