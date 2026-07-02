//! Repository for device-local application state (`app_state`).
//!
//! Currently backs the last-activity heartbeat used by in-terminal idle
//! detection for active timers (issue #155).

use rusqlite::{Connection, OptionalExtension, params};
use time::OffsetDateTime;

use super::{format_timestamp, parse_timestamp};
use crate::Error;

/// Key under which the last-activity heartbeat is stored.
const LAST_ACTIVITY_KEY: &str = "last_activity";

/// Fetch the most recent last-activity heartbeat, if one has been recorded.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if the stored timestamp is invalid.
pub fn get_last_activity(conn: &Connection) -> Result<Option<OffsetDateTime>, Error> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM app_state WHERE key = ?1",
            params![LAST_ACTIVITY_KEY],
            |row| row.get(0),
        )
        .optional()?;
    raw.map(|value| parse_timestamp(&value)).transpose()
}

/// Record `now` as the last-activity heartbeat, replacing any prior value.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures and
/// [`crate::Error::Core`] if the timestamp cannot be formatted.
pub fn touch(conn: &Connection, now: OffsetDateTime) -> Result<(), Error> {
    let now_text = format_timestamp(now)?;
    conn.execute(
        "INSERT INTO app_state (key, value, updated_at)
         VALUES (?1, ?2, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?2",
        params![LAST_ACTIVITY_KEY, now_text],
    )?;
    Ok(())
}

/// Remove the last-activity heartbeat (e.g. once idle has been resolved).
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures.
pub fn clear(conn: &Connection) -> Result<(), Error> {
    conn.execute(
        "DELETE FROM app_state WHERE key = ?1",
        params![LAST_ACTIVITY_KEY],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{clear, get_last_activity, touch};
    use crate::migrations;
    use rusqlite::Connection;
    use time::OffsetDateTime;

    fn conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        migrations::migrate(&mut conn).unwrap();
        conn
    }

    #[test]
    fn absent_heartbeat_is_none() {
        let conn = conn();
        assert!(get_last_activity(&conn).unwrap().is_none());
    }

    #[test]
    fn touch_then_get_roundtrips() {
        let conn = conn();
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        touch(&conn, now).unwrap();
        assert_eq!(get_last_activity(&conn).unwrap(), Some(now));
    }

    #[test]
    fn touch_replaces_prior_value() {
        let conn = conn();
        let first = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let second = OffsetDateTime::from_unix_timestamp(1_700_000_600).unwrap();
        touch(&conn, first).unwrap();
        touch(&conn, second).unwrap();
        assert_eq!(get_last_activity(&conn).unwrap(), Some(second));
    }

    #[test]
    fn clear_removes_heartbeat() {
        let conn = conn();
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        touch(&conn, now).unwrap();
        clear(&conn).unwrap();
        assert!(get_last_activity(&conn).unwrap().is_none());
    }
}
