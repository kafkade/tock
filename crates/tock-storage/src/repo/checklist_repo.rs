//! Repository functions for task checklist items.
//!
//! Checklist items are addressed within the CLI by their 1-based position
//! in the task's ordered list; the repository exposes helpers that resolve
//! such an index to a stored row and keeps `position` values contiguous
//! after removals and reorders.

use rusqlite::{Connection, Row, params};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    format_timestamp, parse_optional_timestamp, parse_timestamp, parse_u32, parse_uuid_blob,
    uuid_to_blob,
};
use crate::Error;
use tock_core::domain::checklist::ChecklistItem;

const SELECT_ITEM_SQL: &str =
    "SELECT id, task_id, title, position, done_at, created_at FROM checklist_items";

/// List a task's checklist items ordered by position.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn list(conn: &Connection, task_id: Uuid) -> Result<Vec<ChecklistItem>, Error> {
    let sql = format!("{SELECT_ITEM_SQL} WHERE task_id = ?1 ORDER BY position ASC");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![uuid_to_blob(task_id)])?;
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        items.push(read_item_row(row)?);
    }
    Ok(items)
}

/// Append a new checklist item to a task and return the stored row.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures and
/// [`crate::Error::Core`] if stored data cannot be decoded.
pub fn add(conn: &Connection, task_id: Uuid, title: &str) -> Result<ChecklistItem, Error> {
    let id = Uuid::now_v7();
    let created_at = OffsetDateTime::now_utc();
    let created_at_text = format_timestamp(created_at)?;
    let next_position: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM checklist_items WHERE task_id = ?1",
        params![uuid_to_blob(task_id)],
        |row| row.get(0),
    )?;

    conn.execute(
        "INSERT INTO checklist_items (id, task_id, title, position, done_at, created_at)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
        params![
            uuid_to_blob(id),
            uuid_to_blob(task_id),
            title,
            next_position,
            created_at_text,
        ],
    )?;

    fetch_by_id(conn, id)?.ok_or(Error::NotFound)
}

/// Mark the item at `index` (1-based) done or not done and return it.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if the index is out of range,
/// [`crate::Error::Sqlite`] on write failures, and [`crate::Error::Core`]
/// on decode failures.
pub fn set_done(
    conn: &Connection,
    task_id: Uuid,
    index: u32,
    done: bool,
) -> Result<ChecklistItem, Error> {
    let item = item_at(conn, task_id, index)?;
    let done_at = if done {
        Some(format_timestamp(OffsetDateTime::now_utc())?)
    } else {
        None
    };
    conn.execute(
        "UPDATE checklist_items SET done_at = ?1 WHERE id = ?2",
        params![done_at, uuid_to_blob(item.id)],
    )?;
    fetch_by_id(conn, item.id)?.ok_or(Error::NotFound)
}

/// Remove the item at `index` (1-based) and resequence remaining positions.
///
/// Returns the removed item's title for user feedback.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if the index is out of range and
/// [`crate::Error::Sqlite`] on write failures.
pub fn remove(conn: &Connection, task_id: Uuid, index: u32) -> Result<String, Error> {
    let item = item_at(conn, task_id, index)?;
    conn.execute(
        "DELETE FROM checklist_items WHERE id = ?1",
        params![uuid_to_blob(item.id)],
    )?;
    resequence(conn, task_id)?;
    Ok(item.title)
}

/// Move the item at `from` (1-based) to position `to` (1-based).
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if either index is out of range and
/// [`crate::Error::Sqlite`] on write failures.
pub fn reorder(conn: &Connection, task_id: Uuid, from: u32, to: u32) -> Result<(), Error> {
    let mut items = list(conn, task_id)?;
    let len = items.len();
    let from_idx = index_to_offset(from, len)?;
    let to_idx = index_to_offset(to, len)?;
    if from_idx == to_idx {
        return Ok(());
    }
    let moved = items.remove(from_idx);
    items.insert(to_idx, moved);

    for (position, item) in items.iter().enumerate() {
        conn.execute(
            "UPDATE checklist_items SET position = ?1 WHERE id = ?2",
            params![
                i64::try_from(position).unwrap_or(i64::MAX),
                uuid_to_blob(item.id)
            ],
        )?;
    }
    Ok(())
}

/// Resolve a 1-based index to the stored item within a task's ordered list.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if the index is out of range.
pub fn item_at(conn: &Connection, task_id: Uuid, index: u32) -> Result<ChecklistItem, Error> {
    let items = list(conn, task_id)?;
    let offset = index_to_offset(index, items.len())?;
    items.into_iter().nth(offset).ok_or(Error::NotFound)
}

const fn index_to_offset(index: u32, len: usize) -> Result<usize, Error> {
    if index == 0 {
        return Err(Error::NotFound);
    }
    let offset = (index - 1) as usize;
    if offset >= len {
        return Err(Error::NotFound);
    }
    Ok(offset)
}

fn resequence(conn: &Connection, task_id: Uuid) -> Result<(), Error> {
    let items = list(conn, task_id)?;
    for (position, item) in items.iter().enumerate() {
        conn.execute(
            "UPDATE checklist_items SET position = ?1 WHERE id = ?2",
            params![
                i64::try_from(position).unwrap_or(i64::MAX),
                uuid_to_blob(item.id)
            ],
        )?;
    }
    Ok(())
}

fn fetch_by_id(conn: &Connection, id: Uuid) -> Result<Option<ChecklistItem>, Error> {
    let sql = format!("{SELECT_ITEM_SQL} WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![uuid_to_blob(id)])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(read_item_row(row)?));
    }
    Ok(None)
}

fn read_item_row(row: &Row<'_>) -> Result<ChecklistItem, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    let task_id_bytes: Vec<u8> = row.get("task_id")?;
    let position: i64 = row.get("position")?;
    Ok(ChecklistItem {
        id: parse_uuid_blob(&id_bytes)?,
        task_id: parse_uuid_blob(&task_id_bytes)?,
        title: row.get("title")?,
        position: parse_u32(position)?,
        done_at: parse_optional_timestamp(row.get::<_, Option<String>>("done_at")?.as_deref())?,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;

    use super::{add, item_at, list, remove, reorder, set_done};
    use crate::migrations;
    use crate::repo::task_repo;
    use tock_core::domain::task::{NewTask, TaskStatus};
    use tock_core::domain::urgency::UrgencyConfig;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    fn task_id(conn: &Connection) -> uuid::Uuid {
        let new_task = NewTask {
            title: String::from("Ship feature"),
            status: Some(TaskStatus::Pending),
            ..NewTask::default()
        };
        task_repo::insert(conn, &new_task, &UrgencyConfig::default())
            .expect("insert task")
            .id
    }

    #[test]
    fn add_appends_in_order() {
        let conn = test_conn();
        let tid = task_id(&conn);
        add(&conn, tid, "first").expect("add");
        add(&conn, tid, "second").expect("add");
        add(&conn, tid, "third").expect("add");

        let items = list(&conn, tid).expect("list");
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["first", "second", "third"]);
        assert_eq!(items[0].position, 0);
        assert_eq!(items[2].position, 2);
    }

    #[test]
    fn check_and_uncheck_toggles_done() {
        let conn = test_conn();
        let tid = task_id(&conn);
        add(&conn, tid, "step").expect("add");

        let checked = set_done(&conn, tid, 1, true).expect("check");
        assert!(checked.is_done());
        let unchecked = set_done(&conn, tid, 1, false).expect("uncheck");
        assert!(!unchecked.is_done());
    }

    #[test]
    fn remove_resequences_positions() {
        let conn = test_conn();
        let tid = task_id(&conn);
        add(&conn, tid, "a").expect("add");
        add(&conn, tid, "b").expect("add");
        add(&conn, tid, "c").expect("add");

        let removed = remove(&conn, tid, 2).expect("remove");
        assert_eq!(removed, "b");

        let items = list(&conn, tid).expect("list");
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["a", "c"]);
        assert_eq!(items[0].position, 0);
        assert_eq!(items[1].position, 1);
    }

    #[test]
    fn reorder_moves_item() {
        let conn = test_conn();
        let tid = task_id(&conn);
        add(&conn, tid, "a").expect("add");
        add(&conn, tid, "b").expect("add");
        add(&conn, tid, "c").expect("add");

        reorder(&conn, tid, 3, 1).expect("reorder");

        let items = list(&conn, tid).expect("list");
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["c", "a", "b"]);
    }

    #[test]
    fn out_of_range_index_is_not_found() {
        let conn = test_conn();
        let tid = task_id(&conn);
        add(&conn, tid, "only").expect("add");

        assert!(matches!(
            item_at(&conn, tid, 0),
            Err(crate::Error::NotFound)
        ));
        assert!(matches!(
            item_at(&conn, tid, 2),
            Err(crate::Error::NotFound)
        ));
    }

    #[test]
    fn checklist_hydrates_into_task() {
        let conn = test_conn();
        let tid = task_id(&conn);
        add(&conn, tid, "one").expect("add");
        add(&conn, tid, "two").expect("add");
        set_done(&conn, tid, 1, true).expect("check");

        let task = task_repo::get_by_id(&conn, tid)
            .expect("fetch")
            .expect("task exists");
        assert_eq!(task.checklist.len(), 2);
        assert!(task.checklist[0].is_done());
        assert!(!task.checklist[1].is_done());
    }

    #[test]
    fn checklist_deleted_with_task() {
        let conn = test_conn();
        let tid = task_id(&conn);
        add(&conn, tid, "temp").expect("add");

        conn.execute(
            "DELETE FROM tasks WHERE id = ?1",
            rusqlite::params![tid.as_bytes().to_vec()],
        )
        .expect("hard delete task");

        assert!(list(&conn, tid).expect("list").is_empty());
    }
}
