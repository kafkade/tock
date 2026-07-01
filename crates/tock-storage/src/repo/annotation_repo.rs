//! Repository functions for annotations (append-only notes on entities).

use rusqlite::{Connection, Row, params};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{format_timestamp, parse_timestamp, parse_uuid_blob, uuid_to_blob};
use crate::Error;
use tock_core::domain::annotation::{Annotation, NewAnnotation};

const SELECT_SQL: &str = "SELECT id, entity_id, entity_kind, body, created_at FROM annotations";

/// Append an annotation to an entity and return the stored row.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn add(conn: &Connection, input: &NewAnnotation) -> Result<Annotation, Error> {
    let id = Uuid::now_v7();
    let created_at = input.created_at.unwrap_or_else(OffsetDateTime::now_utc);
    let created_at_text = format_timestamp(created_at)?;

    conn.execute(
        "INSERT INTO annotations (id, entity_id, entity_kind, body, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            uuid_to_blob(id),
            uuid_to_blob(input.entity_id),
            input.entity_kind,
            input.body,
            created_at_text,
        ],
    )?;

    Ok(Annotation {
        id,
        entity_id: input.entity_id,
        entity_kind: input.entity_kind.clone(),
        body: input.body.clone(),
        created_at,
    })
}

/// Return every annotation on an entity, oldest first.
///
/// Ordering is by `created_at` then `id` so the 1-based positions shown
/// to the user are stable and match removal by index.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored data cannot be decoded.
pub fn list_for_entity(
    conn: &Connection,
    entity_id: Uuid,
    entity_kind: &str,
) -> Result<Vec<Annotation>, Error> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT_SQL} WHERE entity_id = ?1 AND entity_kind = ?2
         ORDER BY created_at ASC, id ASC"
    ))?;
    let mut rows = stmt.query(params![uuid_to_blob(entity_id), entity_kind])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(read_row(row)?);
    }
    Ok(out)
}

/// Count the annotations attached to an entity.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures.
pub fn count_for_entity(
    conn: &Connection,
    entity_id: Uuid,
    entity_kind: &str,
) -> Result<usize, Error> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM annotations WHERE entity_id = ?1 AND entity_kind = ?2",
        params![uuid_to_blob(entity_id), entity_kind],
        |r| r.get(0),
    )?;
    Ok(usize::try_from(count).unwrap_or(0))
}

/// Remove the annotation at `index_1based` (as shown by
/// [`list_for_entity`]) and return the removed row.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if the index is out of range, and
/// [`crate::Error::Sqlite`] on database failures.
pub fn remove_by_index(
    conn: &Connection,
    entity_id: Uuid,
    entity_kind: &str,
    index_1based: usize,
) -> Result<Annotation, Error> {
    let annotations = list_for_entity(conn, entity_id, entity_kind)?;
    let target = index_1based
        .checked_sub(1)
        .and_then(|idx| annotations.get(idx))
        .ok_or(Error::NotFound)?;

    conn.execute(
        "DELETE FROM annotations WHERE id = ?1",
        params![uuid_to_blob(target.id)],
    )?;
    Ok(target.clone())
}

fn read_row(row: &Row<'_>) -> Result<Annotation, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    let entity_bytes: Vec<u8> = row.get("entity_id")?;
    let created_at: String = row.get("created_at")?;
    Ok(Annotation {
        id: parse_uuid_blob(&id_bytes)?,
        entity_id: parse_uuid_blob(&entity_bytes)?,
        entity_kind: row.get("entity_kind")?,
        body: row.get("body")?,
        created_at: parse_timestamp(&created_at)?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::migrations;

    fn conn() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        migrations::migrate(&mut c).unwrap();
        c
    }

    #[test]
    fn add_and_list_in_order() {
        let c = conn();
        let task = Uuid::now_v7();
        add(&c, &NewAnnotation::for_task(task, "first")).unwrap();
        add(&c, &NewAnnotation::for_task(task, "second")).unwrap();

        let list = list_for_entity(&c, task, "task").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].body, "first");
        assert_eq!(list[1].body, "second");
        assert_eq!(count_for_entity(&c, task, "task").unwrap(), 2);
    }

    #[test]
    fn list_is_scoped_to_entity() {
        let c = conn();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        add(&c, &NewAnnotation::for_task(a, "for-a")).unwrap();
        add(&c, &NewAnnotation::for_task(b, "for-b")).unwrap();

        let list = list_for_entity(&c, a, "task").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].body, "for-a");
    }

    #[test]
    fn remove_by_index_deletes_and_returns() {
        let c = conn();
        let task = Uuid::now_v7();
        add(&c, &NewAnnotation::for_task(task, "one")).unwrap();
        add(&c, &NewAnnotation::for_task(task, "two")).unwrap();
        add(&c, &NewAnnotation::for_task(task, "three")).unwrap();

        let removed = remove_by_index(&c, task, "task", 2).unwrap();
        assert_eq!(removed.body, "two");

        let list = list_for_entity(&c, task, "task").unwrap();
        assert_eq!(
            list.iter().map(|a| a.body.as_str()).collect::<Vec<_>>(),
            vec!["one", "three"]
        );
    }

    #[test]
    fn remove_out_of_range_is_not_found() {
        let c = conn();
        let task = Uuid::now_v7();
        add(&c, &NewAnnotation::for_task(task, "only")).unwrap();
        assert!(matches!(
            remove_by_index(&c, task, "task", 2),
            Err(Error::NotFound)
        ));
        assert!(matches!(
            remove_by_index(&c, task, "task", 0),
            Err(Error::NotFound)
        ));
    }

    #[test]
    fn preserves_explicit_created_at() {
        let c = conn();
        let task = Uuid::now_v7();
        let ts = OffsetDateTime::from_unix_timestamp(1_600_000_000).unwrap();
        add(
            &c,
            &NewAnnotation {
                entity_id: task,
                entity_kind: "task".to_string(),
                body: "back-dated".to_string(),
                created_at: Some(ts),
            },
        )
        .unwrap();

        let list = list_for_entity(&c, task, "task").unwrap();
        assert_eq!(list[0].created_at, ts);
    }
}
