//! Repository functions for headings.

use rusqlite::{Connection, Row, params};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{format_timestamp, parse_timestamp, parse_uuid_blob, uuid_to_blob};
use crate::Error;
use tock_core::domain::heading::{Heading, NewHeading};

const SELECT_HEADING_SQL: &str =
    "SELECT id, project_id, name, sort_order, created_at, modified_at FROM headings";

/// Insert a new heading row and return it.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn insert(conn: &Connection, input: &NewHeading) -> Result<Heading, Error> {
    let id = Uuid::now_v7();
    let created_at = OffsetDateTime::now_utc();
    let created_at_text = format_timestamp(created_at)?;

    conn.execute(
        "INSERT INTO headings (id, project_id, name, sort_order, created_at, modified_at)
         VALUES (?1, ?2, ?3, 0, ?4, ?5)",
        params![
            uuid_to_blob(id),
            uuid_to_blob(input.project_id),
            input.name,
            created_at_text,
            created_at_text
        ],
    )?;

    fetch_heading_by_id(conn, id)?.ok_or(Error::NotFound)
}

/// List headings for a project in display order.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn list_for_project(conn: &Connection, project_id: Uuid) -> Result<Vec<Heading>, Error> {
    let sql = format!("{SELECT_HEADING_SQL} WHERE project_id = ?1 ORDER BY sort_order ASC, id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![uuid_to_blob(project_id)])?;
    let mut headings = Vec::new();
    while let Some(row) = rows.next()? {
        headings.push(read_heading_row(row)?);
    }
    Ok(headings)
}

/// Permanently delete a heading row.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures and
/// [`crate::Error::NotFound`] if the heading does not exist.
pub fn delete(conn: &Connection, id: Uuid) -> Result<(), Error> {
    let rows_affected = conn.execute(
        "DELETE FROM headings WHERE id = ?1",
        params![uuid_to_blob(id)],
    )?;

    if rows_affected == 0 {
        return Err(Error::NotFound);
    }

    Ok(())
}

fn fetch_heading_by_id(conn: &Connection, id: Uuid) -> Result<Option<Heading>, Error> {
    let sql = format!("{SELECT_HEADING_SQL} WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![uuid_to_blob(id)])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(read_heading_row(row)?));
    }
    Ok(None)
}

fn read_heading_row(row: &Row<'_>) -> Result<Heading, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    let project_id_bytes: Vec<u8> = row.get("project_id")?;
    Ok(Heading {
        id: parse_uuid_blob(&id_bytes)?,
        project_id: parse_uuid_blob(&project_id_bytes)?,
        name: row.get("name")?,
        sort_order: row.get("sort_order")?,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
        modified_at: parse_timestamp(&row.get::<_, String>("modified_at")?)?,
    })
}
