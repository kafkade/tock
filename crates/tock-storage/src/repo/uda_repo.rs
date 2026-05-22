//! Repository functions for UDA definitions.

use rusqlite::{Connection, params};

use crate::Error;
use crate::repo::invalid_encoding;
use tock_core::domain::uda::{UdaDefinition, UdaType};

/// Insert a new UDA definition.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored type data is invalid.
pub fn add_definition(conn: &Connection, definition: &UdaDefinition) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO uda_definitions (key, type, label, \"default\") VALUES (?1, ?2, ?3, ?4)",
        params![
            definition.key.as_str(),
            definition.uda_type.as_str(),
            definition.label.as_deref(),
            definition.default.as_deref(),
        ],
    )?;
    Ok(())
}

/// List all UDA definitions in key order.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored type data is invalid.
pub fn list_definitions(conn: &Connection) -> Result<Vec<UdaDefinition>, Error> {
    let mut stmt =
        conn.prepare("SELECT key, type, label, \"default\" FROM uda_definitions ORDER BY key ASC")?;
    let mut rows = stmt.query([])?;
    let mut definitions = Vec::new();
    while let Some(row) = rows.next()? {
        definitions.push(read_definition_row(row)?);
    }
    Ok(definitions)
}

/// Remove a UDA definition by key.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures.
pub fn remove_definition(conn: &Connection, key: &str) -> Result<(), Error> {
    conn.execute("DELETE FROM uda_definitions WHERE key = ?1", params![key])?;
    Ok(())
}

/// Fetch a single UDA definition by key.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored type data is invalid.
pub fn get_definition(conn: &Connection, key: &str) -> Result<Option<UdaDefinition>, Error> {
    let mut stmt =
        conn.prepare("SELECT key, type, label, \"default\" FROM uda_definitions WHERE key = ?1")?;
    let mut rows = stmt.query(params![key])?;
    if let Some(row) = rows.next()? {
        Ok(Some(read_definition_row(row)?))
    } else {
        Ok(None)
    }
}

fn read_definition_row(row: &rusqlite::Row<'_>) -> Result<UdaDefinition, Error> {
    let type_raw: String = row.get("type")?;
    let uda_type = UdaType::from_str_opt(&type_raw).ok_or_else(invalid_encoding)?;
    Ok(UdaDefinition {
        key: row.get("key")?,
        uda_type,
        label: row.get("label")?,
        default: row.get("default")?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;
    use tock_core::domain::uda::{UdaDefinition, UdaType};

    use super::{add_definition, get_definition, list_definitions, remove_definition};
    use crate::migrations;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    #[test]
    fn manages_uda_definitions() {
        let conn = test_conn();
        let definition = UdaDefinition {
            key: String::from("effort"),
            uda_type: UdaType::Number,
            label: Some(String::from("Effort")),
            default: Some(String::from("1")),
        };

        add_definition(&conn, &definition).expect("add definition");
        let fetched = get_definition(&conn, "effort")
            .expect("fetch definition")
            .expect("definition exists");
        assert_eq!(fetched, definition);

        let listed = list_definitions(&conn).expect("list definitions");
        assert_eq!(listed, vec![definition]);

        remove_definition(&conn, "effort").expect("remove definition");
        assert!(
            get_definition(&conn, "effort")
                .expect("fetch after removal")
                .is_none()
        );
    }
}
