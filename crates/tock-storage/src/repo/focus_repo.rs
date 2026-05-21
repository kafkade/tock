//! Repository functions for focus sessions.

use rusqlite::{Connection, Row, params};
use time::OffsetDateTime;
use tock_core::domain::focus::{FocusConfig, FocusSession, FocusState, NewFocusSession};
use tock_core::domain::sid::SidKind;
use uuid::Uuid;

use super::{
    format_timestamp, parse_optional_timestamp, parse_optional_uuid_blob, parse_timestamp,
    parse_u32, parse_uuid_blob, uuid_to_blob,
};
use crate::Error;
use crate::repo::sid_repo;

const SELECT_FOCUS_SESSION_SQL: &str = "SELECT id, sid, started_at, ended_at, task_id, project_id, planned_cycles, completed_cycles, state, config_snapshot, created_at, modified_at FROM focus_sessions";

/// Insert a new focus session and return it.
///
/// # Errors
/// Returns [`crate::Error::InvalidState`] when the session configuration is invalid,
/// [`crate::Error::Sqlite`] on write failures, and [`crate::Error::Core`] when stored
/// timestamps or UUIDs cannot be decoded.
pub fn insert(conn: &Connection, new: &NewFocusSession) -> Result<FocusSession, Error> {
    validate_new_session(new)?;

    let id = Uuid::now_v7();
    let sid = sid_repo::next_sid(conn, SidKind::Focus)?;
    let started_at = OffsetDateTime::now_utc();
    let started_at_text = format_timestamp(started_at)?;
    let config_snapshot =
        serde_json::to_string(&new.config).map_err(|_| super::invalid_encoding())?;

    conn.execute(
        "INSERT INTO focus_sessions (
             id, sid, started_at, ended_at, task_id, project_id, planned_cycles,
             completed_cycles, state, config_snapshot, created_at, modified_at
         )
         VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, 0, ?7, ?8, ?9, ?10)",
        params![
            uuid_to_blob(id),
            i64::from(sid),
            started_at_text,
            new.task_id.map(uuid_to_blob),
            new.project_id.map(uuid_to_blob),
            i64::from(new.planned_cycles),
            FocusState::Working.as_str(),
            config_snapshot,
            format_timestamp(started_at)?,
            format_timestamp(started_at)?,
        ],
    )?;

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

/// Fetch the most recently started non-terminal focus session, if any.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and [`crate::Error::Core`]
/// if stored UUID or timestamp data is invalid.
pub fn get_active(conn: &Connection) -> Result<Option<FocusSession>, Error> {
    let sql = format!(
        "{SELECT_FOCUS_SESSION_SQL} WHERE state NOT IN ('aborted', 'completed') ORDER BY started_at DESC LIMIT 1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(read_focus_session_row(row)?));
    }
    Ok(None)
}

/// Fetch a focus session by SID.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and [`crate::Error::Core`]
/// if stored UUID or timestamp data is invalid.
pub fn get_by_sid(conn: &Connection, sid: u32) -> Result<Option<FocusSession>, Error> {
    let sql = format!("{SELECT_FOCUS_SESSION_SQL} WHERE sid = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![i64::from(sid)])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(read_focus_session_row(row)?));
    }
    Ok(None)
}

/// Complete the current work cycle and transition to the next state.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] when the session does not exist,
/// [`crate::Error::InvalidState`] when the session is not currently working,
/// and [`crate::Error::Core`] when stored timestamps or UUIDs are invalid.
pub fn complete_cycle(conn: &Connection, sid: u32) -> Result<FocusSession, Error> {
    let existing = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    if existing.state != FocusState::Working {
        return Err(Error::InvalidState(
            "focus session is not currently working",
        ));
    }

    let completed_cycles = existing
        .completed_cycles
        .checked_add(1)
        .ok_or(Error::InvalidState("focus completed cycles overflow"))?;
    let next_state = next_state_after_cycle(completed_cycles, &existing);
    let modified_at = OffsetDateTime::now_utc();
    let modified_at_text = format_timestamp(modified_at)?;
    let ended_at_text = next_state
        .is_terminal()
        .then(|| format_timestamp(modified_at))
        .transpose()?;

    conn.execute(
        "UPDATE focus_sessions
         SET completed_cycles = ?1,
             state = ?2,
             ended_at = ?3,
             modified_at = ?4
         WHERE sid = ?5",
        params![
            i64::from(completed_cycles),
            next_state.as_str(),
            ended_at_text,
            modified_at_text,
            i64::from(sid),
        ],
    )?;

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

/// Transition from a break back into a work interval.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] when the session does not exist,
/// [`crate::Error::InvalidState`] when the session is not on a break,
/// and [`crate::Error::Core`] when timestamps cannot be formatted.
pub fn start_work(conn: &Connection, sid: u32) -> Result<FocusSession, Error> {
    let existing = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    if !matches!(
        existing.state,
        FocusState::ShortBreak | FocusState::LongBreak
    ) {
        return Err(Error::InvalidState("focus session is not on a break"));
    }

    update_state(conn, sid, FocusState::Working, None)
}

/// Pause an active focus session.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] when the session does not exist,
/// [`crate::Error::InvalidState`] when the session is already paused or terminal,
/// and [`crate::Error::Core`] when timestamps cannot be formatted.
pub fn pause(conn: &Connection, sid: u32) -> Result<FocusSession, Error> {
    let existing = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    if existing.state.is_terminal() || existing.state == FocusState::Paused {
        return Err(Error::InvalidState("focus session cannot be paused"));
    }

    update_state(conn, sid, FocusState::Paused, existing.ended_at)
}

/// Resume a paused focus session.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] when the session does not exist,
/// [`crate::Error::InvalidState`] when the session is not paused,
/// and [`crate::Error::Core`] when timestamps cannot be formatted.
pub fn resume(conn: &Connection, sid: u32) -> Result<FocusSession, Error> {
    let existing = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    if existing.state != FocusState::Paused {
        return Err(Error::InvalidState("focus session is not paused"));
    }

    update_state(conn, sid, FocusState::Working, existing.ended_at)
}

/// Abort a focus session.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] when the session does not exist,
/// [`crate::Error::InvalidState`] when the session is already terminal,
/// and [`crate::Error::Core`] when timestamps cannot be formatted.
pub fn abort(conn: &Connection, sid: u32) -> Result<FocusSession, Error> {
    let existing = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    if existing.state.is_terminal() {
        return Err(Error::InvalidState("focus session is already finished"));
    }

    update_state(
        conn,
        sid,
        FocusState::Aborted,
        Some(OffsetDateTime::now_utc()),
    )
}

/// Mark a focus session as completed.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] when the session does not exist,
/// [`crate::Error::InvalidState`] when the session is already terminal,
/// and [`crate::Error::Core`] when timestamps cannot be formatted.
pub fn finish(conn: &Connection, sid: u32) -> Result<FocusSession, Error> {
    let existing = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    if existing.state.is_terminal() {
        return Err(Error::InvalidState("focus session is already finished"));
    }

    update_state(
        conn,
        sid,
        FocusState::Completed,
        Some(OffsetDateTime::now_utc()),
    )
}

/// List focus sessions whose `started_at` falls within `[from, to)`.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and [`crate::Error::Core`]
/// if stored UUID or timestamp data is invalid.
pub fn list_range(conn: &Connection, from: &str, to: &str) -> Result<Vec<FocusSession>, Error> {
    let sql = format!(
        "{SELECT_FOCUS_SESSION_SQL} WHERE started_at >= ?1 AND started_at < ?2 ORDER BY started_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![from, to])?;
    let mut sessions = Vec::new();
    while let Some(row) = rows.next()? {
        sessions.push(read_focus_session_row(row)?);
    }
    Ok(sessions)
}

const fn validate_new_session(new: &NewFocusSession) -> Result<(), Error> {
    if new.planned_cycles == 0 {
        return Err(Error::InvalidState("focus planned cycles must be positive"));
    }
    if new.config.work_minutes == 0 {
        return Err(Error::InvalidState("focus work minutes must be positive"));
    }
    if new.config.cycles_before_long_break == 0 {
        return Err(Error::InvalidState(
            "focus cycles before long break must be positive",
        ));
    }
    Ok(())
}

const fn next_state_after_cycle(completed_cycles: u32, existing: &FocusSession) -> FocusState {
    if completed_cycles >= existing.planned_cycles {
        FocusState::Completed
    } else if completed_cycles % existing.config.cycles_before_long_break == 0 {
        FocusState::LongBreak
    } else {
        FocusState::ShortBreak
    }
}

fn update_state(
    conn: &Connection,
    sid: u32,
    state: FocusState,
    ended_at: Option<OffsetDateTime>,
) -> Result<FocusSession, Error> {
    let modified_at = OffsetDateTime::now_utc();
    conn.execute(
        "UPDATE focus_sessions
         SET state = ?1,
             ended_at = ?2,
             modified_at = ?3
         WHERE sid = ?4",
        params![
            state.as_str(),
            ended_at.map(format_timestamp).transpose()?,
            format_timestamp(modified_at)?,
            i64::from(sid),
        ],
    )?;

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

fn read_focus_session_row(row: &Row<'_>) -> Result<FocusSession, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    let sid_value: i64 = row.get("sid")?;
    let planned_cycles_raw: i64 = row.get("planned_cycles")?;
    let completed_cycles_raw: i64 = row.get("completed_cycles")?;
    let state_raw: String = row.get("state")?;
    let config_snapshot: String = row.get("config_snapshot")?;

    Ok(FocusSession {
        id: parse_uuid_blob(&id_bytes)?,
        sid: parse_u32(sid_value)?,
        started_at: parse_timestamp(&row.get::<_, String>("started_at")?)?,
        ended_at: parse_optional_timestamp(row.get::<_, Option<String>>("ended_at")?.as_deref())?,
        task_id: parse_optional_uuid_blob(row.get::<_, Option<Vec<u8>>>("task_id")?.as_deref())?,
        project_id: parse_optional_uuid_blob(
            row.get::<_, Option<Vec<u8>>>("project_id")?.as_deref(),
        )?,
        planned_cycles: parse_u32(planned_cycles_raw)?,
        completed_cycles: parse_u32(completed_cycles_raw)?,
        state: parse_focus_state(&state_raw)?,
        config: parse_focus_config(&config_snapshot)?,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
        modified_at: parse_timestamp(&row.get::<_, String>("modified_at")?)?,
    })
}

fn parse_focus_state(raw: &str) -> Result<FocusState, Error> {
    FocusState::from_str_opt(raw).ok_or_else(super::invalid_encoding)
}

fn parse_focus_config(raw: &str) -> Result<FocusConfig, Error> {
    serde_json::from_str(raw).map_err(|_| super::invalid_encoding())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;
    use tock_core::domain::focus::{FocusConfig, FocusState, NewFocusSession};

    use super::{abort, complete_cycle, get_active, insert, list_range, pause, resume, start_work};
    use crate::migrations;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    fn new_session(planned_cycles: u32, cycles_before_long_break: u32) -> NewFocusSession {
        NewFocusSession {
            task_id: None,
            project_id: None,
            planned_cycles,
            config: FocusConfig {
                work_minutes: 25,
                short_break_minutes: 5,
                long_break_minutes: 15,
                cycles_before_long_break,
            },
        }
    }

    #[test]
    fn insert_and_get_active_roundtrip() {
        let conn = test_conn();
        let session = insert(&conn, &new_session(2, 2)).expect("insert focus session");

        assert_eq!(session.state, FocusState::Working);
        assert_eq!(session.completed_cycles, 0);
        assert_eq!(
            get_active(&conn).expect("get active").unwrap().sid,
            session.sid
        );
    }

    #[test]
    fn complete_cycle_transitions_breaks_and_completion() {
        let conn = test_conn();
        let session = insert(&conn, &new_session(3, 2)).expect("insert focus session");

        let session = complete_cycle(&conn, session.sid).expect("complete first cycle");
        assert_eq!(session.completed_cycles, 1);
        assert_eq!(session.state, FocusState::ShortBreak);

        let session = start_work(&conn, session.sid).expect("start second cycle");
        assert_eq!(session.state, FocusState::Working);

        let session = complete_cycle(&conn, session.sid).expect("complete second cycle");
        assert_eq!(session.completed_cycles, 2);
        assert_eq!(session.state, FocusState::LongBreak);

        let session = start_work(&conn, session.sid).expect("start third cycle");
        let session = complete_cycle(&conn, session.sid).expect("complete third cycle");
        assert_eq!(session.completed_cycles, 3);
        assert_eq!(session.state, FocusState::Completed);
        assert!(session.ended_at.is_some());
    }

    #[test]
    fn pause_resume_abort_and_list_range_work() {
        let conn = test_conn();
        let session = insert(&conn, &new_session(2, 2)).expect("insert focus session");

        let session = pause(&conn, session.sid).expect("pause");
        assert_eq!(session.state, FocusState::Paused);
        let session = resume(&conn, session.sid).expect("resume");
        assert_eq!(session.state, FocusState::Working);
        let session = abort(&conn, session.sid).expect("abort");
        assert_eq!(session.state, FocusState::Aborted);
        assert!(get_active(&conn).expect("get active").is_none());

        let sessions =
            list_range(&conn, "2000-01-01T00:00:00Z", "2100-01-01T00:00:00Z").expect("list range");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].sid, session.sid);
    }
}
