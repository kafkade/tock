//! Repository functions for tags and tag associations.

use rusqlite::{Connection, Row, params};
use uuid::Uuid;

use super::{parse_uuid_blob, uuid_to_blob};
use crate::Error;
use tock_core::domain::tag::Tag;

const SELECT_TAG_SQL: &str = "SELECT id, name, color FROM tags";

/// Create the tag if needed and return the stored tag row.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if a stored UUID cannot be decoded.
pub fn ensure_tag(conn: &Connection, name: &str) -> Result<Tag, Error> {
    let id = Uuid::now_v7();
    conn.execute(
        "INSERT OR IGNORE INTO tags (id, name, color) VALUES (?1, ?2, NULL)",
        params![uuid_to_blob(id), name],
    )?;

    fetch_tag_by_name(conn, name)?.ok_or(Error::NotFound)
}

/// Attach the named tag to an entity.
///
/// The tag is created first if it does not already exist.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored UUID data is malformed.
pub fn tag_entity(
    conn: &Connection,
    entity_id: Uuid,
    entity_kind: &str,
    tag_name: &str,
) -> Result<(), Error> {
    let tag = ensure_tag(conn, tag_name)?;
    conn.execute(
        "INSERT OR IGNORE INTO entity_tags (entity_id, entity_kind, tag_id)
         VALUES (?1, ?2, ?3)",
        params![uuid_to_blob(entity_id), entity_kind, uuid_to_blob(tag.id)],
    )?;
    Ok(())
}

/// Remove the named tag from an entity.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures.
pub fn untag_entity(
    conn: &Connection,
    entity_id: Uuid,
    entity_kind: &str,
    tag_name: &str,
) -> Result<(), Error> {
    conn.execute(
        "DELETE FROM entity_tags
         WHERE entity_id = ?1
           AND entity_kind = ?2
           AND tag_id = (SELECT id FROM tags WHERE name = ?3)",
        params![uuid_to_blob(entity_id), entity_kind, tag_name],
    )?;
    Ok(())
}

/// Return all tag names attached to an entity.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures.
pub fn tags_for_entity(
    conn: &Connection,
    entity_id: Uuid,
    entity_kind: &str,
) -> Result<Vec<String>, Error> {
    let mut stmt = conn.prepare(
        "SELECT t.name
         FROM entity_tags AS et
         INNER JOIN tags AS t ON t.id = et.tag_id
         WHERE et.entity_id = ?1 AND et.entity_kind = ?2
         ORDER BY t.name ASC",
    )?;
    let mut rows = stmt.query(params![uuid_to_blob(entity_id), entity_kind])?;
    let mut tags = Vec::new();
    while let Some(row) = rows.next()? {
        tags.push(row.get("name")?);
    }
    Ok(tags)
}

/// Return every tag stored in the vault.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if a stored UUID cannot be decoded.
pub fn list_all(conn: &Connection) -> Result<Vec<Tag>, Error> {
    let mut stmt = conn.prepare(&format!("{SELECT_TAG_SQL} ORDER BY name ASC"))?;
    let mut rows = stmt.query(params![])?;
    let mut tags = Vec::new();
    while let Some(row) = rows.next()? {
        tags.push(read_tag_row(row)?);
    }
    Ok(tags)
}

/// Rename a tag in place.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures and
/// [`crate::Error::NotFound`] if `old` does not exist.
pub fn rename(conn: &Connection, old: &str, new_name: &str) -> Result<(), Error> {
    let rows_affected = conn.execute(
        "UPDATE tags SET name = ?1 WHERE name = ?2",
        params![new_name, old],
    )?;

    if rows_affected == 0 {
        return Err(Error::NotFound);
    }

    Ok(())
}

fn fetch_tag_by_name(conn: &Connection, name: &str) -> Result<Option<Tag>, Error> {
    let sql = format!("{SELECT_TAG_SQL} WHERE name = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![name])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(read_tag_row(row)?));
    }
    Ok(None)
}

fn read_tag_row(row: &Row<'_>) -> Result<Tag, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    Ok(Tag {
        id: parse_uuid_blob(&id_bytes)?,
        name: row.get("name")?,
        color: row.get("color")?,
    })
}
