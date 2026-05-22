//! Repository functions for tasks.

use rusqlite::{Connection, Row, params};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    bool_to_int, format_timestamp, parse_bool, parse_optional_timestamp, parse_optional_uuid_blob,
    parse_timestamp, parse_u32, parse_uuid_blob, uuid_to_blob,
};
use crate::Error;
use crate::repo::{sid_repo, tag_repo};
use tock_core::domain::sid::SidKind;
use tock_core::domain::task::{NewTask, Priority, Task, TaskPatch, TaskStatus};
use tock_core::domain::uda::UdaValues;
use tock_core::domain::urgency::{UrgencyConfig, UrgencyInput, calculate};

const ENTITY_KIND: &str = "task";
const SELECT_TASK_SQL: &str = "SELECT id, sid, title, notes, status, area_id, project_id, heading_id, start_date, deadline, priority, evening, udas, urgency_cache, created_at, modified_at, done_at, cancelled_at, deleted_at FROM tasks";

/// Insert a new task row, attach its tags, and return the stored task.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn insert(conn: &Connection, input: &NewTask) -> Result<Task, Error> {
    let id = Uuid::now_v7();
    let sid = sid_repo::next_sid(conn, SidKind::Task)?;
    let created_at = OffsetDateTime::now_utc();
    let created_at_text = format_timestamp(created_at)?;
    let status = input.status.unwrap_or(TaskStatus::Inbox);
    let (done_at, cancelled_at) = status_timestamps(None, None, status, created_at);

    conn.execute(
        "INSERT INTO tasks (
             id, sid, title, notes, status, area_id, project_id, heading_id,
             start_date, deadline, scheduled_for, evening, priority, udas,
             urgency_cache, created_at, modified_at, done_at, cancelled_at, deleted_at
         )
         VALUES (
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
             ?9, ?10, NULL, ?11, ?12, ?13,
             0.0, ?14, ?15, ?16, ?17, NULL
         )",
        params![
            uuid_to_blob(id),
            i64::from(sid),
            input.title,
            input.notes,
            status.as_str(),
            input.area_id.map(uuid_to_blob),
            input.project_id.map(uuid_to_blob),
            input.heading_id.map(uuid_to_blob),
            input.start_date,
            input.deadline,
            bool_to_int(input.evening),
            input.priority.map(priority_to_storage),
            input.udas.to_json(),
            created_at_text,
            created_at_text,
            done_at.map(format_timestamp).transpose()?,
            cancelled_at.map(format_timestamp).transpose()?,
        ],
    )?;

    for tag_name in &input.tags {
        tag_repo::tag_entity(conn, id, ENTITY_KIND, tag_name)?;
    }
    let _ = recalculate_urgency(conn, sid)?;

    get_by_id(conn, id)?.ok_or(Error::NotFound)
}

/// Fetch a task by SID, including its tags.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn get_by_sid(conn: &Connection, sid: u32) -> Result<Option<Task>, Error> {
    fetch_task(conn, "sid = ?1", params![i64::from(sid)])
}

/// Fetch a task by UUID, including its tags.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn get_by_id(conn: &Connection, id: Uuid) -> Result<Option<Task>, Error> {
    fetch_task(conn, "id = ?1", params![uuid_to_blob(id)])
}

/// List tasks ordered by urgency descending then SID ascending.
///
/// Soft-deleted tasks are excluded unless `include_deleted` is `true`.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn list(conn: &Connection, include_deleted: bool) -> Result<Vec<Task>, Error> {
    let sql = if include_deleted {
        format!("{SELECT_TASK_SQL} ORDER BY urgency_cache DESC, sid ASC")
    } else {
        format!("{SELECT_TASK_SQL} WHERE deleted_at IS NULL ORDER BY urgency_cache DESC, sid ASC")
    };

    let mut tasks = Vec::new();
    {
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![])?;
        while let Some(row) = rows.next()? {
            tasks.push(read_task_row(row)?);
        }
    }

    for task in &mut tasks {
        task.tags = tag_repo::tags_for_entity(conn, task.id, ENTITY_KIND)?;
    }

    Ok(tasks)
}

/// Apply a patch to an existing task and return the updated row.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures,
/// [`crate::Error::NotFound`] if the task does not exist, and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn update(conn: &Connection, sid: u32, patch: &TaskPatch) -> Result<Task, Error> {
    let existing = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    let now = OffsetDateTime::now_utc();
    let now_text = format_timestamp(now)?;

    let title = patch
        .title
        .clone()
        .unwrap_or_else(|| existing.title.clone());
    let notes = patch
        .notes
        .clone()
        .unwrap_or_else(|| existing.notes.clone());
    let status = patch.status.unwrap_or(existing.status);
    let area_id = patch.area_id.unwrap_or(existing.area_id);
    let project_id = patch.project_id.unwrap_or(existing.project_id);
    let heading_id = patch.heading_id.unwrap_or(existing.heading_id);
    let start_date = patch
        .start_date
        .clone()
        .unwrap_or_else(|| existing.start_date.clone());
    let deadline = patch
        .deadline
        .clone()
        .unwrap_or_else(|| existing.deadline.clone());
    let priority = patch.priority.unwrap_or(existing.priority);
    let evening = patch.evening.unwrap_or(existing.evening);
    let mut udas = existing.udas.clone();
    for (key, value) in &patch.set_udas {
        udas.set(key, value.clone());
    }
    for key in &patch.remove_udas {
        udas.remove(key);
    }
    let udas_json = udas.to_json();
    let (done_at, cancelled_at) = if patch.status.is_some() {
        status_timestamps(existing.done_at, existing.cancelled_at, status, now)
    } else {
        (existing.done_at, existing.cancelled_at)
    };

    conn.execute(
        "UPDATE tasks
         SET title = ?1,
             notes = ?2,
             status = ?3,
             area_id = ?4,
             project_id = ?5,
             heading_id = ?6,
             start_date = ?7,
             deadline = ?8,
             priority = ?9,
             evening = ?10,
             udas = ?11,
             modified_at = ?12,
             done_at = ?13,
             cancelled_at = ?14
         WHERE sid = ?15",
        params![
            title,
            notes,
            status.as_str(),
            area_id.map(uuid_to_blob),
            project_id.map(uuid_to_blob),
            heading_id.map(uuid_to_blob),
            start_date,
            deadline,
            priority.map(priority_to_storage),
            bool_to_int(evening),
            udas_json,
            now_text,
            done_at.map(format_timestamp).transpose()?,
            cancelled_at.map(format_timestamp).transpose()?,
            i64::from(sid),
        ],
    )?;

    for tag_name in &patch.add_tags {
        tag_repo::tag_entity(conn, existing.id, ENTITY_KIND, tag_name)?;
    }
    for tag_name in &patch.remove_tags {
        tag_repo::untag_entity(conn, existing.id, ENTITY_KIND, tag_name)?;
    }
    let _ = recalculate_urgency(conn, sid)?;

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

/// Change a task's status and return the updated row.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures,
/// [`crate::Error::NotFound`] if the task does not exist, and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn set_status(conn: &Connection, sid: u32, status: TaskStatus) -> Result<Task, Error> {
    let existing = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    let now = OffsetDateTime::now_utc();
    let now_text = format_timestamp(now)?;
    let (done_at, cancelled_at) =
        status_timestamps(existing.done_at, existing.cancelled_at, status, now);

    conn.execute(
        "UPDATE tasks
         SET status = ?1,
             modified_at = ?2,
             done_at = ?3,
             cancelled_at = ?4
         WHERE sid = ?5",
        params![
            status.as_str(),
            now_text,
            done_at.map(format_timestamp).transpose()?,
            cancelled_at.map(format_timestamp).transpose()?,
            i64::from(sid),
        ],
    )?;
    let _ = recalculate_urgency(conn, sid)?;

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

/// Recalculate and persist a task's cached urgency score.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if the task does not exist and
/// [`crate::Error::Sqlite`] for read or write failures.
pub fn recalculate_urgency(conn: &Connection, sid: u32) -> Result<f64, Error> {
    let task = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    let now = OffsetDateTime::now_utc();
    let today = urgency_today(now);
    let input = UrgencyInput {
        priority: task.priority.map(|priority| priority.as_char()),
        deadline: task.deadline.as_deref(),
        start_date: task.start_date.as_deref(),
        tags: &task.tags,
        has_project: task.project_id.is_some(),
        created_at_days_ago: task_age_days(task.created_at, now),
        today: &today,
    };
    let score = calculate(&input, &UrgencyConfig::default());
    conn.execute(
        "UPDATE tasks SET urgency_cache = ?1 WHERE sid = ?2",
        params![score, i64::from(sid)],
    )?;
    Ok(score)
}

/// Soft-delete a task by setting `deleted_at`.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures,
/// [`crate::Error::NotFound`] if the task does not exist, and
/// [`crate::Error::Core`] if the timestamp cannot be formatted.
pub fn soft_delete(conn: &Connection, sid: u32) -> Result<(), Error> {
    let now_text = format_timestamp(OffsetDateTime::now_utc())?;
    let rows_affected = conn.execute(
        "UPDATE tasks SET deleted_at = ?1, modified_at = ?2 WHERE sid = ?3",
        params![now_text, now_text, i64::from(sid)],
    )?;

    if rows_affected == 0 {
        return Err(Error::NotFound);
    }

    Ok(())
}

fn fetch_task<P>(conn: &Connection, filter: &str, params: P) -> Result<Option<Task>, Error>
where
    P: rusqlite::Params,
{
    let sql = format!("{SELECT_TASK_SQL} WHERE {filter}");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params)?;
    if let Some(row) = rows.next()? {
        let mut task = read_task_row(row)?;
        task.tags = tag_repo::tags_for_entity(conn, task.id, ENTITY_KIND)?;
        return Ok(Some(task));
    }
    Ok(None)
}

fn task_age_days(created_at: OffsetDateTime, now: OffsetDateTime) -> f64 {
    let days = (now - created_at).whole_days().clamp(0, 365);
    f64::from(u16::try_from(days).unwrap_or(365))
}

fn urgency_today(now: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    )
}

fn read_task_row(row: &Row<'_>) -> Result<Task, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    let sid_value: i64 = row.get("sid")?;
    let status_raw: String = row.get("status")?;
    let priority_raw: Option<String> = row.get("priority")?;
    let evening_raw: i64 = row.get("evening")?;
    let udas_raw: String = row.get("udas")?;

    Ok(Task {
        id: parse_uuid_blob(&id_bytes)?,
        sid: parse_u32(sid_value)?,
        title: row.get("title")?,
        notes: row.get("notes")?,
        status: parse_task_status(&status_raw)?,
        area_id: parse_optional_uuid_blob(row.get::<_, Option<Vec<u8>>>("area_id")?.as_deref())?,
        project_id: parse_optional_uuid_blob(
            row.get::<_, Option<Vec<u8>>>("project_id")?.as_deref(),
        )?,
        heading_id: parse_optional_uuid_blob(
            row.get::<_, Option<Vec<u8>>>("heading_id")?.as_deref(),
        )?,
        start_date: row.get("start_date")?,
        deadline: row.get("deadline")?,
        priority: priority_raw.as_deref().map(parse_priority).transpose()?,
        evening: parse_bool(evening_raw)?,
        udas: UdaValues::from_json(&udas_raw),
        tags: Vec::new(),
        urgency: row.get("urgency_cache")?,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
        modified_at: parse_timestamp(&row.get::<_, String>("modified_at")?)?,
        done_at: parse_optional_timestamp(row.get::<_, Option<String>>("done_at")?.as_deref())?,
        cancelled_at: parse_optional_timestamp(
            row.get::<_, Option<String>>("cancelled_at")?.as_deref(),
        )?,
        deleted_at: parse_optional_timestamp(
            row.get::<_, Option<String>>("deleted_at")?.as_deref(),
        )?,
    })
}

fn parse_task_status(raw: &str) -> Result<TaskStatus, Error> {
    TaskStatus::from_str_opt(raw).ok_or_else(super::invalid_encoding)
}

fn parse_priority(raw: &str) -> Result<Priority, Error> {
    Priority::from_str_opt(raw).ok_or_else(super::invalid_encoding)
}

fn priority_to_storage(priority: Priority) -> String {
    priority.as_char().to_string()
}

fn status_timestamps(
    current_done_at: Option<OffsetDateTime>,
    current_cancelled_at: Option<OffsetDateTime>,
    next_status: TaskStatus,
    now: OffsetDateTime,
) -> (Option<OffsetDateTime>, Option<OffsetDateTime>) {
    let done_at = if next_status == TaskStatus::Done {
        current_done_at.or(Some(now))
    } else {
        None
    };
    let cancelled_at = if next_status == TaskStatus::Cancelled {
        current_cancelled_at.or(Some(now))
    } else {
        None
    };
    (done_at, cancelled_at)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;
    use tock_core::domain::task::{NewTask, Priority, TaskPatch, TaskStatus};

    use super::{get_by_sid, insert, update};
    use crate::migrations;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    fn sample_new_task() -> NewTask {
        let mut task = NewTask {
            title: String::from("Write tests"),
            status: Some(TaskStatus::Pending),
            ..NewTask::default()
        };
        task.udas.set("effort", serde_json::json!(3));
        task
    }

    #[test]
    fn inserts_and_reads_udas() {
        let conn = test_conn();
        let task = insert(&conn, &sample_new_task()).expect("insert task");

        assert_eq!(task.udas.get_str("effort").as_deref(), Some("3"));
        let fetched = get_by_sid(&conn, task.sid)
            .expect("fetch task")
            .expect("task exists");
        assert_eq!(fetched.udas.get_str("effort").as_deref(), Some("3"));
    }

    #[test]
    fn update_merges_and_removes_udas() {
        let conn = test_conn();
        let task = insert(&conn, &sample_new_task()).expect("insert task");

        let mut patch = TaskPatch::default();
        patch
            .set_udas
            .insert(String::from("owner"), serde_json::json!("sam"));
        patch
            .set_udas
            .insert(String::from("effort"), serde_json::json!(5));
        patch.remove_udas.push(String::from("missing"));
        let updated = update(&conn, task.sid, &patch).expect("update task");
        assert_eq!(updated.udas.get_str("effort").as_deref(), Some("5"));
        assert_eq!(updated.udas.get_str("owner").as_deref(), Some("sam"));

        let mut remove_patch = TaskPatch::default();
        remove_patch.remove_udas.push(String::from("owner"));
        let updated = update(&conn, task.sid, &remove_patch).expect("remove uda");
        assert_eq!(updated.udas.get_str("effort").as_deref(), Some("5"));
        assert_eq!(updated.udas.get_str("owner"), None);
    }

    #[test]
    fn recalculates_urgency_when_tasks_change() {
        let conn = test_conn();
        let mut new_task = sample_new_task();
        new_task.priority = Some(Priority::Low);
        let task = insert(&conn, &new_task).expect("insert task");
        let baseline_urgency = task.urgency;

        let mut patch = TaskPatch {
            priority: Some(Some(Priority::High)),
            ..TaskPatch::default()
        };
        patch.add_tags.push(String::from("next"));
        let updated = update(&conn, task.sid, &patch).expect("update task urgency");

        assert!(baseline_urgency > 0.0);
        assert!(updated.urgency > baseline_urgency);
    }
}
