//! Repository functions for named task contexts.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Error;
use crate::repo::invalid_encoding;

const ACTIVE_CONTEXT_KEY: &str = "active_context";

/// Define or replace a named context filter.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures.
pub fn define(conn: &Connection, name: &str, filter: &str) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO contexts (name, filter) VALUES (?1, ?2)
         ON CONFLICT(name) DO UPDATE SET filter = excluded.filter",
        params![name, filter],
    )?;
    Ok(())
}

/// List all contexts in name order.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures.
pub fn list(conn: &Connection) -> Result<Vec<(String, String)>, Error> {
    let mut stmt = conn.prepare("SELECT name, filter FROM contexts ORDER BY name ASC")?;
    let mut rows = stmt.query([])?;
    let mut contexts = Vec::new();
    while let Some(row) = rows.next()? {
        contexts.push((row.get("name")?, row.get("filter")?));
    }
    Ok(contexts)
}

/// Delete a named context.
///
/// If the deleted context was active, the active context setting is cleared.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures.
pub fn delete(conn: &Connection, name: &str) -> Result<(), Error> {
    conn.execute("DELETE FROM contexts WHERE name = ?1", params![name])?;
    if get_active(conn)?.as_deref() == Some(name) {
        set_active(conn, None)?;
    }
    Ok(())
}

/// Set the active context name.
///
/// Passing `None` clears the active context.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if `name` does not exist and
/// [`crate::Error::Sqlite`] on database failures.
pub fn set_active(conn: &Connection, name: Option<&str>) -> Result<(), Error> {
    match name {
        Some(name) => {
            if get_filter(conn, name)?.is_none() {
                return Err(Error::NotFound);
            }
            conn.execute(
                "INSERT OR REPLACE INTO vault_meta (key, value) VALUES (?1, ?2)",
                params![ACTIVE_CONTEXT_KEY, name.as_bytes()],
            )?;
        }
        None => {
            conn.execute(
                "DELETE FROM vault_meta WHERE key = ?1",
                params![ACTIVE_CONTEXT_KEY],
            )?;
        }
    }
    Ok(())
}

/// Get the active context name, if one is set.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if the stored UTF-8 data is invalid.
pub fn get_active(conn: &Connection) -> Result<Option<String>, Error> {
    let raw: Option<Vec<u8>> = conn
        .query_row(
            "SELECT value FROM vault_meta WHERE key = ?1",
            params![ACTIVE_CONTEXT_KEY],
            |row| row.get(0),
        )
        .optional()?;
    raw.map(|value| String::from_utf8(value).map_err(|_| invalid_encoding()))
        .transpose()
}

/// Fetch a context filter expression by name.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures.
pub fn get_filter(conn: &Connection, name: &str) -> Result<Option<String>, Error> {
    conn.query_row(
        "SELECT filter FROM contexts WHERE name = ?1",
        params![name],
        |row| row.get(0),
    )
    .optional()
    .map_err(Error::from)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;

    use super::{define, delete, get_active, get_filter, list, set_active};
    use crate::migrations;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    #[test]
    fn defines_lists_and_activates_contexts() {
        let conn = test_conn();
        define(&conn, "work", "tag:work status:pending").expect("define context");
        define(&conn, "home", "tag:home").expect("define context");

        let contexts = list(&conn).expect("list contexts");
        assert_eq!(contexts.len(), 2);
        assert_eq!(contexts[0].0, "home");
        assert_eq!(contexts[1].0, "work");
        assert_eq!(
            get_filter(&conn, "work").expect("get work filter"),
            Some(String::from("tag:work status:pending"))
        );

        set_active(&conn, Some("work")).expect("set active context");
        assert_eq!(
            get_active(&conn).expect("get active context"),
            Some(String::from("work"))
        );

        set_active(&conn, None).expect("clear active context");
        assert_eq!(get_active(&conn).expect("get active context"), None);
    }

    #[test]
    fn deleting_active_context_clears_setting() {
        let conn = test_conn();
        define(&conn, "work", "tag:work").expect("define context");
        set_active(&conn, Some("work")).expect("set active context");

        delete(&conn, "work").expect("delete context");
        assert!(
            get_filter(&conn, "work")
                .expect("get deleted filter")
                .is_none()
        );
        assert!(get_active(&conn).expect("get active context").is_none());
    }
}
