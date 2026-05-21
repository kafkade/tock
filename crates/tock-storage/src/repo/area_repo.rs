//! Repository functions for areas.

use rusqlite::{Connection, Row, params};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{format_timestamp, parse_optional_timestamp, parse_uuid_blob, uuid_to_blob};
use crate::Error;
use tock_core::domain::area::{Area, NewArea};

const SELECT_AREA_SQL: &str =
    "SELECT id, name, color, sort_order, created_at, modified_at, archived_at FROM areas";

/// Insert a new area row and return it.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn insert(conn: &Connection, input: &NewArea) -> Result<Area, Error> {
    let id = Uuid::now_v7();
    let created_at = OffsetDateTime::now_utc();
    let created_at_text = format_timestamp(created_at)?;

    conn.execute(
        "INSERT INTO areas (id, name, color, sort_order, archived_at, created_at, modified_at)
         VALUES (?1, ?2, ?3, 0, NULL, ?4, ?5)",
        params![
            uuid_to_blob(id),
            input.name,
            input.color,
            created_at_text,
            created_at_text,
        ],
    )?;

    fetch_area_by_id(conn, id)?.ok_or(Error::NotFound)
}

/// List stored areas.
///
/// Archived areas are excluded unless `include_archived` is `true`.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn list(conn: &Connection, include_archived: bool) -> Result<Vec<Area>, Error> {
    let sql = if include_archived {
        format!("{SELECT_AREA_SQL} ORDER BY sort_order ASC, name ASC")
    } else {
        format!("{SELECT_AREA_SQL} WHERE archived_at IS NULL ORDER BY sort_order ASC, name ASC")
    };

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![])?;
    let mut areas = Vec::new();
    while let Some(row) = rows.next()? {
        areas.push(read_area_row(row)?);
    }
    Ok(areas)
}

/// Archive an area by UUID.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures,
/// [`crate::Error::NotFound`] if the area does not exist, and
/// [`crate::Error::Core`] if the timestamp cannot be formatted.
pub fn archive(conn: &Connection, id: Uuid) -> Result<(), Error> {
    let now_text = format_timestamp(OffsetDateTime::now_utc())?;
    let rows_affected = conn.execute(
        "UPDATE areas SET archived_at = ?1, modified_at = ?2 WHERE id = ?3",
        params![now_text, now_text, uuid_to_blob(id)],
    )?;

    if rows_affected == 0 {
        return Err(Error::NotFound);
    }

    Ok(())
}

fn fetch_area_by_id(conn: &Connection, id: Uuid) -> Result<Option<Area>, Error> {
    let sql = format!("{SELECT_AREA_SQL} WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![uuid_to_blob(id)])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(read_area_row(row)?));
    }
    Ok(None)
}

fn read_area_row(row: &Row<'_>) -> Result<Area, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    Ok(Area {
        id: parse_uuid_blob(&id_bytes)?,
        name: row.get("name")?,
        color: row.get("color")?,
        sort_order: row.get("sort_order")?,
        created_at: super::parse_timestamp(&row.get::<_, String>("created_at")?)?,
        modified_at: super::parse_timestamp(&row.get::<_, String>("modified_at")?)?,
        archived_at: parse_optional_timestamp(
            row.get::<_, Option<String>>("archived_at")?.as_deref(),
        )?,
    })
}
