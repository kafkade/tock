//! Repository functions for time blocks.

use rusqlite::{Connection, Row, params};
use time::OffsetDateTime;
use tock_core::domain::sid::SidKind;
use tock_core::domain::time_block::{BlockSource, NewTimeBlock, TimeBlock};
use uuid::Uuid;

use super::{
    bool_to_int, format_timestamp, parse_bool, parse_optional_timestamp, parse_optional_uuid_blob,
    parse_timestamp, parse_u32, parse_uuid_blob, uuid_to_blob,
};
use crate::Error;
use crate::repo::sid_repo;

const SELECT_TIME_BLOCK_SQL: &str = "SELECT id, sid, title, start_ts, end_ts, project_id, task_id, notes, source, billable, created_at, modified_at FROM time_blocks";

/// Insert a new running time block and return it.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn insert(conn: &Connection, new: &NewTimeBlock) -> Result<TimeBlock, Error> {
    insert_block(
        conn,
        &InsertTimeBlockInput {
            title: &new.title,
            task_id: new.task_id,
            project_id: new.project_id,
            notes: new.notes.as_deref(),
            source: new.source,
            billable: false,
        },
        OffsetDateTime::now_utc(),
        None,
    )
}

/// Insert a closed time block with explicit timestamps and return it.
///
/// # Errors
/// Returns [`crate::Error::InvalidState`] when `end_ts` precedes `start_ts`,
/// [`crate::Error::Sqlite`] on write failures, and [`crate::Error::Core`] if
/// stored UUID or timestamp data is invalid.
pub fn insert_completed(
    conn: &Connection,
    new: &NewTimeBlock,
    start_ts: OffsetDateTime,
    end_ts: OffsetDateTime,
) -> Result<TimeBlock, Error> {
    if end_ts < start_ts {
        return Err(Error::InvalidState("time block end precedes start"));
    }

    insert_block(
        conn,
        &InsertTimeBlockInput {
            title: &new.title,
            task_id: new.task_id,
            project_id: new.project_id,
            notes: new.notes.as_deref(),
            source: new.source,
            billable: false,
        },
        start_ts,
        Some(end_ts),
    )
}

/// Stop the running time block identified by `sid` and return the updated row.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if the block does not exist,
/// [`crate::Error::InvalidState`] if it is already stopped, and
/// [`crate::Error::Core`] if the timestamp cannot be formatted.
pub fn stop(conn: &Connection, sid: u32) -> Result<TimeBlock, Error> {
    let existing = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    if !existing.is_running() {
        return Err(Error::InvalidState("time block is already stopped"));
    }

    let now_text = format_timestamp(OffsetDateTime::now_utc())?;
    let rows_affected = conn.execute(
        "UPDATE time_blocks
         SET end_ts = ?1,
             modified_at = ?2
         WHERE sid = ?3 AND end_ts IS NULL",
        params![now_text, now_text, i64::from(sid)],
    )?;

    if rows_affected == 0 {
        return Err(Error::InvalidState("time block is already stopped"));
    }

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

/// Fetch the currently running block, if any.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn get_current(conn: &Connection) -> Result<Option<TimeBlock>, Error> {
    let sql =
        format!("{SELECT_TIME_BLOCK_SQL} WHERE end_ts IS NULL ORDER BY start_ts DESC LIMIT 1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(read_time_block_row(row)?));
    }
    Ok(None)
}

/// Fetch a time block by SID.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn get_by_sid(conn: &Connection, sid: u32) -> Result<Option<TimeBlock>, Error> {
    let sql = format!("{SELECT_TIME_BLOCK_SQL} WHERE sid = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![i64::from(sid)])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(read_time_block_row(row)?));
    }
    Ok(None)
}

/// Patch fields on an existing time block.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if no block with the given SID
/// exists.
pub fn update(
    conn: &Connection,
    sid: u32,
    patch: &tock_core::domain::time_block::TimeBlockPatch,
) -> Result<TimeBlock, Error> {
    let _existing = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    let now = format_timestamp(OffsetDateTime::now_utc())?;

    let mut sets = vec!["modified_at = ?1".to_string()];
    let mut idx: usize = 2;

    macro_rules! push_set {
        ($field:expr) => {{
            sets.push(format!("{} = ?{idx}", $field));
            idx += 1;
        }};
    }

    if patch.title.is_some() {
        push_set!("title");
    }
    if patch.notes.is_some() {
        push_set!("notes");
    }
    if patch.start.is_some() {
        push_set!("start_ts");
    }
    if patch.end.is_some() {
        push_set!("end_ts");
    }
    if patch.task_id.is_some() {
        push_set!("task_id");
    }
    if patch.billable.is_some() {
        push_set!("billable");
    }

    let sql = format!(
        "UPDATE time_blocks SET {} WHERE sid = ?{idx}",
        sets.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;

    let mut bind_idx: usize = 1;
    stmt.raw_bind_parameter(bind_idx, &now)?;
    bind_idx += 1;

    if let Some(ref t) = patch.title {
        stmt.raw_bind_parameter(bind_idx, t.as_str())?;
        bind_idx += 1;
    }
    if let Some(ref n) = patch.notes {
        stmt.raw_bind_parameter(bind_idx, n.as_deref())?;
        bind_idx += 1;
    }
    if let Some(ref s) = patch.start {
        stmt.raw_bind_parameter(bind_idx, s.as_str())?;
        bind_idx += 1;
    }
    if let Some(ref e) = patch.end {
        stmt.raw_bind_parameter(bind_idx, e.as_deref())?;
        bind_idx += 1;
    }
    if let Some(ref tid) = patch.task_id {
        stmt.raw_bind_parameter(bind_idx, tid.map(uuid_to_blob))?;
        bind_idx += 1;
    }
    if let Some(b) = patch.billable {
        stmt.raw_bind_parameter(bind_idx, bool_to_int(b))?;
        bind_idx += 1;
    }
    stmt.raw_bind_parameter(bind_idx, i64::from(sid))?;
    stmt.raw_execute()?;

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

/// List time blocks for a specific task, ordered by start descending.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures.
pub fn list_for_task(conn: &Connection, task_id: Uuid) -> Result<Vec<TimeBlock>, Error> {
    let sql = format!("{SELECT_TIME_BLOCK_SQL} WHERE task_id = ?1 ORDER BY start_ts DESC");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![uuid_to_blob(task_id)])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(read_time_block_row(row)?);
    }
    Ok(out)
}

/// List time blocks ordered by start timestamp descending.
///
/// Running blocks are included only when `include_running` is `true`.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn list(conn: &Connection, include_running: bool) -> Result<Vec<TimeBlock>, Error> {
    let sql = if include_running {
        format!("{SELECT_TIME_BLOCK_SQL} ORDER BY start_ts DESC")
    } else {
        format!("{SELECT_TIME_BLOCK_SQL} WHERE end_ts IS NOT NULL ORDER BY start_ts DESC")
    };

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![])?;
    let mut blocks = Vec::new();
    while let Some(row) = rows.next()? {
        blocks.push(read_time_block_row(row)?);
    }
    Ok(blocks)
}

/// List time blocks whose `start_ts` falls within `[from, to)`.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn list_range(conn: &Connection, from: &str, to: &str) -> Result<Vec<TimeBlock>, Error> {
    let sql = format!(
        "{SELECT_TIME_BLOCK_SQL} WHERE start_ts >= ?1 AND start_ts < ?2 ORDER BY start_ts DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![from, to])?;
    let mut blocks = Vec::new();
    while let Some(row) = rows.next()? {
        blocks.push(read_time_block_row(row)?);
    }
    Ok(blocks)
}

/// Resume the most recently stopped time block by creating a new running copy.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if no stopped block exists,
/// [`crate::Error::InvalidState`] if a block is already running, and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn resume(conn: &Connection) -> Result<TimeBlock, Error> {
    if get_current(conn)?.is_some() {
        return Err(Error::InvalidState("a time block is already running"));
    }

    let sql =
        format!("{SELECT_TIME_BLOCK_SQL} WHERE end_ts IS NOT NULL ORDER BY end_ts DESC LIMIT 1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![])?;
    let Some(row) = rows.next()? else {
        return Err(Error::NotFound);
    };
    let previous = read_time_block_row(row)?;

    insert_block(
        conn,
        &InsertTimeBlockInput {
            title: &previous.title,
            task_id: previous.task_id,
            project_id: previous.project_id,
            notes: previous.notes.as_deref(),
            source: previous.source,
            billable: previous.billable,
        },
        OffsetDateTime::now_utc(),
        None,
    )
}

/// Hard-delete a time block by SID.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures and
/// [`crate::Error::NotFound`] if the block does not exist.
pub fn delete(conn: &Connection, sid: u32) -> Result<(), Error> {
    let rows_affected = conn.execute(
        "DELETE FROM time_blocks WHERE sid = ?1",
        params![i64::from(sid)],
    )?;

    if rows_affected == 0 {
        return Err(Error::NotFound);
    }

    Ok(())
}

struct InsertTimeBlockInput<'a> {
    title: &'a str,
    task_id: Option<Uuid>,
    project_id: Option<Uuid>,
    notes: Option<&'a str>,
    source: BlockSource,
    billable: bool,
}

fn insert_block(
    conn: &Connection,
    input: &InsertTimeBlockInput<'_>,
    start_ts: OffsetDateTime,
    end_ts: Option<OffsetDateTime>,
) -> Result<TimeBlock, Error> {
    let id = Uuid::now_v7();
    let sid = sid_repo::next_sid(conn, SidKind::Block)?;
    let recorded_at = OffsetDateTime::now_utc();

    conn.execute(
        "INSERT INTO time_blocks (
             id, sid, title, start_ts, end_ts, project_id, task_id, notes,
             source, billable, created_at, modified_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            uuid_to_blob(id),
            i64::from(sid),
            input.title,
            format_timestamp(start_ts)?,
            end_ts.map(format_timestamp).transpose()?,
            input.project_id.map(uuid_to_blob),
            input.task_id.map(uuid_to_blob),
            input.notes,
            input.source.as_str(),
            bool_to_int(input.billable),
            format_timestamp(recorded_at)?,
            format_timestamp(recorded_at)?,
        ],
    )?;

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

fn read_time_block_row(row: &Row<'_>) -> Result<TimeBlock, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    let sid_value: i64 = row.get("sid")?;
    let source_raw: String = row.get("source")?;
    let billable_raw: i64 = row.get("billable")?;

    Ok(TimeBlock {
        id: parse_uuid_blob(&id_bytes)?,
        sid: parse_u32(sid_value)?,
        title: row.get("title")?,
        start_ts: parse_timestamp(&row.get::<_, String>("start_ts")?)?,
        end_ts: parse_optional_timestamp(row.get::<_, Option<String>>("end_ts")?.as_deref())?,
        project_id: parse_optional_uuid_blob(
            row.get::<_, Option<Vec<u8>>>("project_id")?.as_deref(),
        )?,
        task_id: parse_optional_uuid_blob(row.get::<_, Option<Vec<u8>>>("task_id")?.as_deref())?,
        notes: row.get("notes")?,
        source: parse_block_source(&source_raw)?,
        billable: parse_bool(billable_raw)?,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
        modified_at: parse_timestamp(&row.get::<_, String>("modified_at")?)?,
    })
}

fn parse_block_source(raw: &str) -> Result<BlockSource, Error> {
    BlockSource::from_str_opt(raw).ok_or_else(super::invalid_encoding)
}
