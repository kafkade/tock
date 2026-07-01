//! Generic snapshot/diff undo-redo journal (issue #150).
//!
//! Domain repositories in this crate write directly to `SQLite` and never
//! emit per-operation events — the append-only event log is only
//! synthesized at sync time (see [`crate::sync`]). To make mutating CLI
//! commands reversible we therefore capture a **row-level before/after
//! diff** around each command instead of replaying events.
//!
//! ## How it works
//!
//! 1. [`snapshot`] reads every row of a curated set of mutable domain
//!    tables into typed [`Cell`] values (primary-key columns are read
//!    from `PRAGMA table_info` at runtime).
//! 2. After the command runs, another snapshot is taken and [`record`]
//!    diffs the two by primary key to produce a [`ChangeSet`] of
//!    inserts / updates / deletes, persisting it on the undo stack.
//! 3. [`undo`] applies a change set's inverse (delete inserted rows,
//!    restore the before-image of updated/deleted rows); [`redo`]
//!    re-applies it forward. Both run inside a single transaction with
//!    deferred foreign keys so restore order never trips FK enforcement.
//!
//! The journal is **device-local**: the `undo_log` table is deliberately
//! excluded from the sync registry, so undo history never leaves the
//! device. Undo/redo is a linear stack — recording a new entry discards
//! any pending redo entries.

use rusqlite::types::Value;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::Error;

/// Maximum number of undo entries retained; older entries are pruned.
const MAX_ENTRIES: i64 = 200;

/// Mutable domain tables tracked for undo/redo, ordered parents-before-
/// children so a forward restore is well-formed under foreign keys.
///
/// Local-only bookkeeping (sync state, journals, device registry,
/// migrations, event log) and the `undo_log` itself are intentionally
/// excluded, as are `sid_counters` (monotonic allocators whose advance
/// is harmless to leave in place).
const TRACKED_TABLES: &[&str] = &[
    "areas",
    "projects",
    "headings",
    "tags",
    "tasks",
    "entity_tags",
    "task_dependencies",
    "annotations",
    "time_blocks",
    "focus_sessions",
    "habits",
    "habit_entries",
    "habit_skips",
    "uda_definitions",
    "saved_reports",
    "contexts",
    "caldav_links",
];

/// A single column value, serialized in the change set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Cell {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl From<Value> for Cell {
    fn from(v: Value) -> Self {
        match v {
            Value::Null => Self::Null,
            Value::Integer(i) => Self::Int(i),
            Value::Real(r) => Self::Real(r),
            Value::Text(t) => Self::Text(t),
            Value::Blob(b) => Self::Blob(b),
        }
    }
}

impl From<&Cell> for Value {
    fn from(c: &Cell) -> Self {
        match c {
            Cell::Null => Self::Null,
            Cell::Int(i) => Self::Integer(*i),
            Cell::Real(r) => Self::Real(*r),
            Cell::Text(t) => Self::Text(t.clone()),
            Cell::Blob(b) => Self::Blob(b.clone()),
        }
    }
}

/// One row: ordered `(column, value)` pairs (schema column order).
type Row = Vec<(String, Cell)>;

/// All tracked rows keyed by `table -> primary-key -> row`.
type Snapshot = std::collections::HashMap<String, std::collections::HashMap<String, Row>>;

/// A single row change captured by the diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Change {
    table: String,
    /// Row image before the command (`None` for inserts).
    before: Option<Row>,
    /// Row image after the command (`None` for deletes).
    after: Option<Row>,
}

/// The set of row changes produced by one mutating command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ChangeSet {
    changes: Vec<Change>,
}

impl ChangeSet {
    const fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Outcome of an [`undo`] / [`redo`] call.
#[derive(Debug, Clone)]
pub struct UndoOutcome {
    /// The human label recorded for the reverted / re-applied command.
    pub label: String,
}

// ── Snapshot & diff ──────────────────────────────────────────────────

/// Primary-key column names for `table`, in key order, via
/// `PRAGMA table_info`. Falls back to all columns if the table declares
/// no primary key (none of the tracked tables do).
fn primary_key_columns(conn: &Connection, table: &str) -> Result<Vec<String>, Error> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    // columns: cid, name, type, notnull, dflt_value, pk
    let rows = stmt.query_map([], |r| {
        let name: String = r.get(1)?;
        let pk: i64 = r.get(5)?;
        Ok((name, pk))
    })?;
    let mut keyed: Vec<(i64, String)> = Vec::new();
    let mut all: Vec<String> = Vec::new();
    for row in rows {
        let (name, pk) = row?;
        all.push(name.clone());
        if pk > 0 {
            keyed.push((pk, name));
        }
    }
    if keyed.is_empty() {
        return Ok(all);
    }
    keyed.sort_by_key(|(order, _)| *order);
    Ok(keyed.into_iter().map(|(_, name)| name).collect())
}

/// Build a stable string key from a row's primary-key cell values.
fn row_key(row: &Row, pk_cols: &[String]) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(pk_cols.len());
    for col in pk_cols {
        let cell = row.iter().find(|(c, _)| c == col).map(|(_, v)| v);
        parts.push(match cell {
            Some(Cell::Null) | None => "\u{0}null".to_owned(),
            Some(Cell::Int(i)) => format!("i{i}"),
            Some(Cell::Real(r)) => format!("r{r}"),
            Some(Cell::Text(t)) => format!("t{t}"),
            Some(Cell::Blob(b)) => format!("b{}", hex(b)),
        });
    }
    parts.join("\u{1}")
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Snapshot every tracked table into an in-memory map.
///
/// # Errors
/// [`Error::Sqlite`] on query failure.
pub fn snapshot(conn: &Connection) -> Result<SnapshotHandle, Error> {
    let mut out: Snapshot = std::collections::HashMap::new();
    for &table in TRACKED_TABLES {
        if !table_exists(conn, table) {
            continue;
        }
        let pk_cols = primary_key_columns(conn, table)?;
        let mut stmt = conn.prepare(&format!("SELECT * FROM {table}"))?;
        let col_names: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
        let mut rows = stmt.query([])?;
        let mut table_map: std::collections::HashMap<String, Row> =
            std::collections::HashMap::new();
        while let Some(r) = rows.next()? {
            let mut row: Row = Vec::with_capacity(col_names.len());
            for (idx, name) in col_names.iter().enumerate() {
                let v: Value = r.get(idx)?;
                row.push((name.clone(), Cell::from(v)));
            }
            let key = row_key(&row, &pk_cols);
            table_map.insert(key, row);
        }
        out.insert(table.to_owned(), table_map);
    }
    Ok(SnapshotHandle(out))
}

/// Opaque snapshot captured by [`snapshot`], consumed by [`record`].
pub struct SnapshotHandle(Snapshot);

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
        params![table],
        |r| r.get::<_, i64>(0),
    )
    .ok()
    .is_some()
}

/// Diff `before` against `after`, producing the ordered change set.
fn diff(before: &Snapshot, after: &Snapshot) -> ChangeSet {
    let mut changes = Vec::new();
    for &table in TRACKED_TABLES {
        let empty = std::collections::HashMap::new();
        let b = before.get(table).unwrap_or(&empty);
        let a = after.get(table).unwrap_or(&empty);
        if b.is_empty() && a.is_empty() {
            continue;
        }
        // Inserts and updates.
        for (key, after_row) in a {
            match b.get(key) {
                None => changes.push(Change {
                    table: table.to_owned(),
                    before: None,
                    after: Some(after_row.clone()),
                }),
                Some(before_row) if before_row != after_row => changes.push(Change {
                    table: table.to_owned(),
                    before: Some(before_row.clone()),
                    after: Some(after_row.clone()),
                }),
                Some(_) => {}
            }
        }
        // Deletes.
        for (key, before_row) in b {
            if !a.contains_key(key) {
                changes.push(Change {
                    table: table.to_owned(),
                    before: Some(before_row.clone()),
                    after: None,
                });
            }
        }
    }
    ChangeSet { changes }
}

// ── Record / undo / redo ─────────────────────────────────────────────

/// Diff a before/after snapshot pair and, if anything changed, push a new
/// entry onto the undo stack (clearing any pending redo entries).
///
/// Returns `true` if an entry was recorded.
///
/// # Errors
/// [`Error::Sqlite`] on persistence failure or
/// [`Error::EventLogIntegrity`] on change-set (de)serialization failure.
pub fn record(
    conn: &Connection,
    label: &str,
    before: SnapshotHandle,
    after: SnapshotHandle,
) -> Result<bool, Error> {
    let SnapshotHandle(before) = before;
    let SnapshotHandle(after) = after;
    let change_set = diff(&before, &after);
    if change_set.is_empty() {
        return Ok(false);
    }
    // Recording a new action invalidates the redo stack.
    conn.execute("DELETE FROM undo_log WHERE undone = 1", [])?;

    let blob = serde_json::to_vec(&change_set).map_err(|_| Error::EventLogIntegrity)?;
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default();
    conn.execute(
        "INSERT INTO undo_log (label, changes, undone, created_at) VALUES (?1, ?2, 0, ?3)",
        params![label, blob, now],
    )?;
    prune(conn)?;
    Ok(true)
}

/// Drop the oldest applied entries beyond [`MAX_ENTRIES`].
fn prune(conn: &Connection) -> Result<(), Error> {
    conn.execute(
        "DELETE FROM undo_log
         WHERE undone = 0
           AND seq NOT IN (
               SELECT seq FROM undo_log WHERE undone = 0
               ORDER BY seq DESC LIMIT ?1
           )",
        params![MAX_ENTRIES],
    )?;
    Ok(())
}

/// Revert the most recent applied command. Returns `None` when the undo
/// stack is empty.
///
/// # Errors
/// [`Error::Sqlite`] on failure or [`Error::EventLogIntegrity`] on a
/// corrupt change set.
pub fn undo(conn: &mut Connection) -> Result<Option<UndoOutcome>, Error> {
    let entry = conn
        .query_row(
            "SELECT seq, label, changes FROM undo_log
             WHERE undone = 0 ORDER BY seq DESC LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            },
        )
        .ok();
    let Some((seq, label, blob)) = entry else {
        return Ok(None);
    };
    let change_set: ChangeSet =
        serde_json::from_slice(&blob).map_err(|_| Error::EventLogIntegrity)?;
    apply(conn, &change_set, Direction::Undo)?;
    conn.execute(
        "UPDATE undo_log SET undone = 1 WHERE seq = ?1",
        params![seq],
    )?;
    Ok(Some(UndoOutcome { label }))
}

/// Re-apply the most recently undone command. Returns `None` when the
/// redo stack is empty.
///
/// # Errors
/// [`Error::Sqlite`] on failure or [`Error::EventLogIntegrity`] on a
/// corrupt change set.
pub fn redo(conn: &mut Connection) -> Result<Option<UndoOutcome>, Error> {
    let entry = conn
        .query_row(
            "SELECT seq, label, changes FROM undo_log
             WHERE undone = 1 ORDER BY seq DESC LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            },
        )
        .ok();
    let Some((seq, label, blob)) = entry else {
        return Ok(None);
    };
    let change_set: ChangeSet =
        serde_json::from_slice(&blob).map_err(|_| Error::EventLogIntegrity)?;
    apply(conn, &change_set, Direction::Redo)?;
    conn.execute(
        "UPDATE undo_log SET undone = 0 WHERE seq = ?1",
        params![seq],
    )?;
    Ok(Some(UndoOutcome { label }))
}

#[derive(Clone, Copy)]
enum Direction {
    Undo,
    Redo,
}

/// Apply a change set in the given direction inside one transaction.
/// Foreign keys are deferred so restore order never trips enforcement.
fn apply(conn: &mut Connection, set: &ChangeSet, dir: Direction) -> Result<(), Error> {
    let tx = conn.transaction()?;
    tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
    // Undo walks changes in reverse to unwind children before parents.
    let ordered: Vec<&Change> = match dir {
        Direction::Undo => set.changes.iter().rev().collect(),
        Direction::Redo => set.changes.iter().collect(),
    };
    for change in ordered {
        let pk_cols = primary_key_columns(&tx, &change.table)?;
        let (target, restore) = match dir {
            // Undo: remove the `after` image, put back the `before` image.
            Direction::Undo => (change.after.as_ref(), change.before.as_ref()),
            // Redo: remove the `before` image, put back the `after` image.
            Direction::Redo => (change.before.as_ref(), change.after.as_ref()),
        };
        if let Some(row) = target {
            delete_row(&tx, &change.table, row, &pk_cols)?;
        }
        if let Some(row) = restore {
            insert_row(&tx, &change.table, row)?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn delete_row(conn: &Connection, table: &str, row: &Row, pk_cols: &[String]) -> Result<(), Error> {
    let mut where_parts = Vec::with_capacity(pk_cols.len());
    let mut vals: Vec<Value> = Vec::with_capacity(pk_cols.len());
    for col in pk_cols {
        let cell = row.iter().find(|(c, _)| c == col).map(|(_, v)| v);
        match cell {
            Some(Cell::Null) | None => where_parts.push(format!("{col} IS NULL")),
            Some(v) => {
                where_parts.push(format!("{col} = ?{}", vals.len() + 1));
                vals.push(Value::from(v));
            }
        }
    }
    let sql = format!("DELETE FROM {table} WHERE {}", where_parts.join(" AND "));
    conn.execute(&sql, rusqlite::params_from_iter(vals.iter()))?;
    Ok(())
}

fn insert_row(conn: &Connection, table: &str, row: &Row) -> Result<(), Error> {
    let cols: Vec<&str> = row.iter().map(|(c, _)| c.as_str()).collect();
    let placeholders: Vec<String> = (1..=cols.len()).map(|i| format!("?{i}")).collect();
    let vals: Vec<Value> = row.iter().map(|(_, v)| Value::from(v)).collect();
    let sql = format!(
        "INSERT OR REPLACE INTO {table} ({}) VALUES ({})",
        cols.join(", "),
        placeholders.join(", ")
    );
    conn.execute(&sql, rusqlite::params_from_iter(vals.iter()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::migrations;
    use crate::repo::task_repo;
    use tock_core::domain::task::{NewTask, TaskPatch, TaskStatus};
    use tock_core::domain::urgency::UrgencyConfig;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    fn new_task(title: &str) -> NewTask {
        NewTask {
            title: title.to_owned(),
            status: Some(TaskStatus::Pending),
            ..NewTask::default()
        }
    }

    fn capture<F>(conn: &Connection, label: &str, f: F) -> bool
    where
        F: FnOnce(&Connection),
    {
        let before = snapshot(conn).expect("before");
        f(conn);
        let after = snapshot(conn).expect("after");
        record(conn, label, before, after).expect("record")
    }

    #[test]
    fn undo_redo_add() {
        let mut conn = test_conn();
        let recorded = capture(&conn, "add", |c| {
            task_repo::insert(c, &new_task("first"), &UrgencyConfig::default()).expect("insert");
        });
        assert!(recorded);
        assert_eq!(task_repo::list(&conn, false).expect("list").len(), 1);

        let out = undo(&mut conn).expect("undo").expect("some");
        assert_eq!(out.label, "add");
        assert_eq!(task_repo::list(&conn, false).expect("list").len(), 0);

        let out = redo(&mut conn).expect("redo").expect("some");
        assert_eq!(out.label, "add");
        let tasks = task_repo::list(&conn, false).expect("list");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "first");
    }

    #[test]
    fn undo_redo_modify() {
        let mut conn = test_conn();
        let task =
            task_repo::insert(&conn, &new_task("orig"), &UrgencyConfig::default()).expect("insert");
        let sid = task.sid;

        let recorded = capture(&conn, "modify", |c| {
            let patch = TaskPatch {
                title: Some("changed".to_owned()),
                ..TaskPatch::default()
            };
            task_repo::update(c, sid, &patch, &UrgencyConfig::default()).expect("update");
        });
        assert!(recorded);
        assert_eq!(
            task_repo::get_by_sid(&conn, sid)
                .expect("get")
                .expect("some")
                .title,
            "changed"
        );

        undo(&mut conn).expect("undo").expect("some");
        assert_eq!(
            task_repo::get_by_sid(&conn, sid)
                .expect("get")
                .expect("some")
                .title,
            "orig"
        );

        redo(&mut conn).expect("redo").expect("some");
        assert_eq!(
            task_repo::get_by_sid(&conn, sid)
                .expect("get")
                .expect("some")
                .title,
            "changed"
        );
    }

    #[test]
    fn undo_redo_done() {
        let mut conn = test_conn();
        let task =
            task_repo::insert(&conn, &new_task("todo"), &UrgencyConfig::default()).expect("insert");
        let sid = task.sid;

        capture(&conn, "done", |c| {
            task_repo::set_status(c, sid, TaskStatus::Done, &UrgencyConfig::default())
                .expect("done");
        });
        assert_eq!(
            task_repo::get_by_sid(&conn, sid)
                .expect("get")
                .expect("some")
                .status,
            TaskStatus::Done
        );

        undo(&mut conn).expect("undo").expect("some");
        assert_eq!(
            task_repo::get_by_sid(&conn, sid)
                .expect("get")
                .expect("some")
                .status,
            TaskStatus::Pending
        );
    }

    #[test]
    fn undo_redo_delete() {
        let mut conn = test_conn();
        let task =
            task_repo::insert(&conn, &new_task("gone"), &UrgencyConfig::default()).expect("insert");
        let sid = task.sid;

        capture(&conn, "delete", |c| {
            task_repo::soft_delete(c, sid).expect("delete");
        });
        assert!(
            task_repo::get_by_sid(&conn, sid)
                .expect("get")
                .expect("some")
                .deleted_at
                .is_some()
        );

        undo(&mut conn).expect("undo").expect("some");
        assert!(
            task_repo::get_by_sid(&conn, sid)
                .expect("get")
                .expect("some")
                .deleted_at
                .is_none()
        );
    }

    #[test]
    fn empty_diff_records_nothing() {
        let mut conn = test_conn();
        let recorded = capture(&conn, "noop", |_| {});
        assert!(!recorded);
        assert!(undo(&mut conn).expect("undo").is_none());
    }

    #[test]
    fn new_action_clears_redo_stack() {
        let mut conn = test_conn();
        capture(&conn, "add-1", |c| {
            task_repo::insert(c, &new_task("one"), &UrgencyConfig::default()).expect("insert");
        });
        undo(&mut conn).expect("undo").expect("some");
        // A brand-new action after an undo must invalidate the redo entry.
        capture(&conn, "add-2", |c| {
            task_repo::insert(c, &new_task("two"), &UrgencyConfig::default()).expect("insert");
        });
        assert!(redo(&mut conn).expect("redo").is_none());
    }

    #[test]
    fn nothing_to_undo_or_redo() {
        let mut conn = test_conn();
        assert!(undo(&mut conn).expect("undo").is_none());
        assert!(redo(&mut conn).expect("redo").is_none());
    }

    #[test]
    fn undo_redo_time_block() {
        use crate::repo::time_block_repo;
        use tock_core::domain::time_block::{BlockSource, NewTimeBlock};

        let mut conn = test_conn();
        let recorded = capture(&conn, "time start", |c| {
            time_block_repo::insert(
                c,
                &NewTimeBlock {
                    title: "deep work".to_owned(),
                    task_id: None,
                    project_id: None,
                    notes: None,
                    source: BlockSource::Timer,
                },
            )
            .expect("insert block");
        });
        assert!(recorded);
        assert_eq!(time_block_repo::list(&conn, true).expect("list").len(), 1);

        undo(&mut conn).expect("undo").expect("some");
        assert_eq!(time_block_repo::list(&conn, true).expect("list").len(), 0);

        redo(&mut conn).expect("redo").expect("some");
        assert_eq!(time_block_repo::list(&conn, true).expect("list").len(), 1);
    }

    #[test]
    fn undo_stack_survives_reopen() {
        use tempfile::tempdir;

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("v.tockvault");
        let (vault, sk) = crate::init(&path, b"pw").expect("init");
        {
            let conn = vault.connection();
            let recorded = capture(conn, "add", |c| {
                task_repo::insert(c, &new_task("persisted"), &UrgencyConfig::default())
                    .expect("insert");
            });
            assert!(recorded);
        }
        vault.lock();

        // Reopen the on-disk vault; the undo entry must still be there.
        let mut vault2 = crate::open(&path, b"pw", &sk).expect("open");
        let conn = vault2.connection_mut();
        let out = undo(conn).expect("undo").expect("some");
        assert_eq!(out.label, "add");
        assert_eq!(task_repo::list(conn, false).expect("list").len(), 0);
    }
}
