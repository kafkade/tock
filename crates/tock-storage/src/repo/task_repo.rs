//! Repository functions for tasks.

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, Row, params};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    bool_to_int, format_timestamp, parse_bool, parse_optional_timestamp, parse_optional_uuid_blob,
    parse_timestamp, parse_u32, parse_uuid_blob, uuid_to_blob,
};
use crate::Error;
use crate::repo::{sid_repo, tag_repo};
use tock_core::domain::recurrence::RecurrenceSpec;
use tock_core::domain::sid::SidKind;
use tock_core::domain::task::{NewTask, Priority, Task, TaskPatch, TaskStatus};
use tock_core::domain::uda::UdaValues;
use tock_core::domain::urgency::{UrgencyConfig, UrgencyInput, calculate};

const ENTITY_KIND: &str = "task";
const SELECT_TASK_SQL: &str = "SELECT id, sid, title, notes, status, area_id, project_id, heading_id, parent_id, start_date, deadline, recurrence, priority, evening, udas, urgency_cache, created_at, modified_at, done_at, cancelled_at, deleted_at FROM tasks";

/// Insert a new task row, attach its tags, and return the stored task.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn insert(conn: &Connection, input: &NewTask, urgency: &UrgencyConfig) -> Result<Task, Error> {
    let id = Uuid::now_v7();
    let sid = sid_repo::next_sid(conn, SidKind::Task)?;
    let created_at = OffsetDateTime::now_utc();
    let created_at_text = format_timestamp(created_at)?;
    let status = input.status.unwrap_or(TaskStatus::Inbox);
    let (done_at, cancelled_at) = status_timestamps(None, None, status, created_at);

    conn.execute(
        "INSERT INTO tasks (
             id, sid, title, notes, status, area_id, project_id, heading_id,
             parent_id, start_date, deadline, scheduled_for, recurrence, evening, priority, udas,
             urgency_cache, created_at, modified_at, done_at, cancelled_at, deleted_at
         )
         VALUES (
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
             ?9, ?10, ?11, NULL, ?12, ?13, ?14, ?15,
             0.0, ?16, ?17, ?18, ?19, NULL
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
            input.parent_id.map(uuid_to_blob),
            input.start_date,
            input.deadline,
            input.recurrence,
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
    let _ = recalculate_urgency(conn, sid, urgency)?;

    get_by_id(conn, id)?.ok_or(Error::NotFound)
}

/// Add a dependency edge between two tasks.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if either SID is missing,
/// [`crate::Error::InvalidState`] if the dependency would be circular or too deep,
/// and [`crate::Error::Sqlite`] on database failures.
pub fn add_dependency(
    conn: &Connection,
    task_sid: u32,
    depends_on_sid: u32,
    urgency: &UrgencyConfig,
) -> Result<(), Error> {
    let task_uuid = task_id_for_sid(conn, task_sid)?;
    let dependency_uuid = task_id_for_sid(conn, depends_on_sid)?;
    ensure_no_circular_dependency(conn, task_uuid, dependency_uuid)?;

    conn.execute(
        "INSERT OR IGNORE INTO task_dependencies (task_id, depends_on_id) VALUES (?1, ?2)",
        params![uuid_to_blob(task_uuid), uuid_to_blob(dependency_uuid)],
    )?;
    let _ = recalculate_urgency(conn, task_sid, urgency)?;
    Ok(())
}

/// Remove a dependency edge between two tasks.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::NotFound`] if the task SID does not exist.
pub fn remove_dependency(
    conn: &Connection,
    task_sid: u32,
    depends_on_sid: u32,
    urgency: &UrgencyConfig,
) -> Result<(), Error> {
    let task_uuid = task_id_for_sid(conn, task_sid)?;
    let dependency_uuid = task_id_for_sid(conn, depends_on_sid)?;

    conn.execute(
        "DELETE FROM task_dependencies WHERE task_id = ?1 AND depends_on_id = ?2",
        params![uuid_to_blob(task_uuid), uuid_to_blob(dependency_uuid)],
    )?;
    let _ = recalculate_urgency(conn, task_sid, urgency)?;
    Ok(())
}

/// Return all dependency UUIDs for a task.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored UUID data is invalid.
pub fn get_dependencies(conn: &Connection, task_id: Uuid) -> Result<Vec<Uuid>, Error> {
    let mut stmt = conn.prepare(
        "SELECT depends_on_id FROM task_dependencies WHERE task_id = ?1 ORDER BY rowid ASC",
    )?;
    let mut rows = stmt.query(params![uuid_to_blob(task_id)])?;
    let mut dependencies = Vec::new();
    while let Some(row) = rows.next()? {
        dependencies.push(parse_uuid_blob(&row.get::<_, Vec<u8>>(0)?)?);
    }
    Ok(dependencies)
}

/// Return all task UUIDs that depend on a task.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored UUID data is invalid.
pub fn get_dependents(conn: &Connection, task_id: Uuid) -> Result<Vec<Uuid>, Error> {
    let mut stmt = conn.prepare(
        "SELECT task_id FROM task_dependencies WHERE depends_on_id = ?1 ORDER BY rowid ASC",
    )?;
    let mut rows = stmt.query(params![uuid_to_blob(task_id)])?;
    let mut dependents = Vec::new();
    while let Some(row) = rows.next()? {
        dependents.push(parse_uuid_blob(&row.get::<_, Vec<u8>>(0)?)?);
    }
    Ok(dependents)
}

/// Return whether a task has any unmet dependency.
///
/// Dependencies in `done` or `cancelled` status are considered satisfied.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures.
pub fn is_blocked(conn: &Connection, task_id: Uuid) -> Result<bool, Error> {
    let blocked: i64 = conn.query_row(
        "SELECT EXISTS(
             SELECT 1
             FROM task_dependencies td
             JOIN tasks dep ON dep.id = td.depends_on_id
             WHERE td.task_id = ?1
               AND dep.status NOT IN ('done', 'cancelled')
         )",
        params![uuid_to_blob(task_id)],
        |row| row.get(0),
    )?;
    Ok(blocked != 0)
}

/// Fetch a task by SID, including its tags and dependencies.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn get_by_sid(conn: &Connection, sid: u32) -> Result<Option<Task>, Error> {
    fetch_task(conn, "sid = ?1", params![i64::from(sid)])
}

/// Fetch a task by UUID, including its tags and dependencies.
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
            tasks.push(read_task_row(conn, row)?);
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
pub fn update(
    conn: &Connection,
    sid: u32,
    patch: &TaskPatch,
    urgency: &UrgencyConfig,
) -> Result<Task, Error> {
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
    for dependency_sid in &patch.add_deps {
        add_dependency(conn, sid, *dependency_sid, urgency)?;
    }
    for dependency_sid in &patch.remove_deps {
        remove_dependency(conn, sid, *dependency_sid, urgency)?;
    }
    let _ = recalculate_urgency(conn, sid, urgency)?;
    if patch.status.is_some() {
        recalculate_dependents_urgency(conn, existing.id, urgency)?;
    }

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

/// Change a task's status and return the updated row.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures,
/// [`crate::Error::NotFound`] if the task does not exist,
/// [`crate::Error::InvalidState`] if the recurrence metadata is invalid,
/// and [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn set_status(
    conn: &Connection,
    sid: u32,
    status: TaskStatus,
    urgency: &UrgencyConfig,
) -> Result<Task, Error> {
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
    let _ = recalculate_urgency(conn, sid, urgency)?;
    recalculate_dependents_urgency(conn, existing.id, urgency)?;

    if status == TaskStatus::Done && existing.status != TaskStatus::Done {
        let _ = spawn_next_recurrence(conn, &existing, now, urgency)?;
    }

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

/// Recalculate and persist a task's cached urgency score.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if the task does not exist and
/// [`crate::Error::Sqlite`] for read or write failures.
pub fn recalculate_urgency(
    conn: &Connection,
    sid: u32,
    urgency: &UrgencyConfig,
) -> Result<f64, Error> {
    let task = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    let now = OffsetDateTime::now_utc();
    let today = urgency_today(now);
    let input = UrgencyInput {
        priority: task.priority.map(|priority| priority.as_char()),
        deadline: task.deadline.as_deref(),
        start_date: task.start_date.as_deref(),
        tags: &task.tags,
        has_project: task.project_id.is_some(),
        is_blocked: is_blocked(conn, task.id)?,
        created_at_days_ago: task_age_days(task.created_at, now),
        today: &today,
    };
    let score = calculate(&input, urgency);
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
        let mut task = read_task_row(conn, row)?;
        task.tags = tag_repo::tags_for_entity(conn, task.id, ENTITY_KIND)?;
        return Ok(Some(task));
    }
    Ok(None)
}

fn read_task_row(conn: &Connection, row: &Row<'_>) -> Result<Task, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    let id = parse_uuid_blob(&id_bytes)?;
    let sid_value: i64 = row.get("sid")?;
    let status_raw: String = row.get("status")?;
    let priority_raw: Option<String> = row.get("priority")?;
    let evening_raw: i64 = row.get("evening")?;
    let udas_raw: String = row.get("udas")?;

    Ok(Task {
        id,
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
        parent_id: parse_optional_uuid_blob(
            row.get::<_, Option<Vec<u8>>>("parent_id")?.as_deref(),
        )?,
        start_date: row.get("start_date")?,
        deadline: row.get("deadline")?,
        recurrence: row.get("recurrence")?,
        priority: priority_raw.as_deref().map(parse_priority).transpose()?,
        evening: parse_bool(evening_raw)?,
        udas: UdaValues::from_json(&udas_raw),
        tags: Vec::new(),
        depends_on: get_dependencies(conn, id)?,
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

fn completion_date(now: OffsetDateTime) -> String {
    urgency_today(now)
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

fn task_id_for_sid(conn: &Connection, sid: u32) -> Result<Uuid, Error> {
    let raw: Option<Vec<u8>> = conn
        .query_row(
            "SELECT id FROM tasks WHERE sid = ?1",
            params![i64::from(sid)],
            |row| row.get(0),
        )
        .optional()?;
    raw.as_deref()
        .map(parse_uuid_blob)
        .transpose()?
        .ok_or(Error::NotFound)
}

fn task_sid_for_id(conn: &Connection, id: Uuid) -> Result<Option<u32>, Error> {
    let sid: Option<i64> = conn
        .query_row(
            "SELECT sid FROM tasks WHERE id = ?1",
            params![uuid_to_blob(id)],
            |row| row.get(0),
        )
        .optional()?;
    sid.map(parse_u32).transpose()
}

fn ensure_no_circular_dependency(
    conn: &Connection,
    task_id: Uuid,
    depends_on_id: Uuid,
) -> Result<(), Error> {
    if task_id == depends_on_id {
        return Err(Error::InvalidState("circular dependency"));
    }

    let mut stack = vec![(depends_on_id, 0_usize)];
    let mut visited = HashSet::new();
    while let Some((current, depth)) = stack.pop() {
        if current == task_id {
            return Err(Error::InvalidState("circular dependency"));
        }
        if depth >= 100 {
            return Err(Error::InvalidState("dependency chain too deep"));
        }
        if !visited.insert(current) {
            continue;
        }
        for dependency in get_dependencies(conn, current)? {
            stack.push((dependency, depth + 1));
        }
    }
    Ok(())
}

fn recalculate_dependents_urgency(
    conn: &Connection,
    task_id: Uuid,
    urgency: &UrgencyConfig,
) -> Result<(), Error> {
    for dependent_id in get_dependents(conn, task_id)? {
        if let Some(sid) = task_sid_for_id(conn, dependent_id)? {
            let _ = recalculate_urgency(conn, sid, urgency)?;
        }
    }
    Ok(())
}

fn spawn_next_recurrence(
    conn: &Connection,
    existing: &Task,
    completed_at: OffsetDateTime,
    urgency: &UrgencyConfig,
) -> Result<Option<Task>, Error> {
    let Some(recurrence_json) = existing.recurrence.as_deref() else {
        return Ok(None);
    };
    let spec = RecurrenceSpec::from_json(recurrence_json)
        .ok_or(Error::InvalidState("invalid recurrence spec"))?;
    let completed_on = completion_date(completed_at);
    let next_deadline = spec
        .next_date(
            existing.deadline.as_deref().unwrap_or(&completed_on),
            &completed_on,
        )
        .ok_or(Error::InvalidState("invalid recurrence date"))?;
    let template_id = existing.parent_id.unwrap_or(existing.id);
    let next_task = insert(
        conn,
        &NewTask {
            title: existing.title.clone(),
            notes: existing.notes.clone(),
            status: Some(TaskStatus::Pending),
            project_id: existing.project_id,
            area_id: existing.area_id,
            heading_id: existing.heading_id,
            parent_id: Some(template_id),
            start_date: None,
            deadline: Some(next_deadline.clone()),
            recurrence: existing.recurrence.clone(),
            priority: existing.priority,
            evening: existing.evening,
            udas: existing.udas.clone(),
            tags: existing.tags.clone(),
        },
        urgency,
    )?;
    println!(
        "  (created recurring task #{} due {})",
        next_task.sid, next_deadline
    );
    Ok(Some(next_task))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;
    use tock_core::domain::task::{NewTask, Priority, TaskPatch, TaskStatus};

    use super::{
        add_dependency, get_by_sid, insert, is_blocked, remove_dependency, set_status, update,
    };
    use crate::Error;
    use crate::migrations;
    use tock_core::domain::urgency::UrgencyConfig;

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
        let task =
            insert(&conn, &sample_new_task(), &UrgencyConfig::default()).expect("insert task");

        assert_eq!(task.udas.get_str("effort").as_deref(), Some("3"));
        let fetched = get_by_sid(&conn, task.sid)
            .expect("fetch task")
            .expect("task exists");
        assert_eq!(fetched.udas.get_str("effort").as_deref(), Some("3"));
    }

    #[test]
    fn update_merges_and_removes_udas() {
        let conn = test_conn();
        let task =
            insert(&conn, &sample_new_task(), &UrgencyConfig::default()).expect("insert task");

        let mut patch = TaskPatch::default();
        patch
            .set_udas
            .insert(String::from("owner"), serde_json::json!("sam"));
        patch
            .set_udas
            .insert(String::from("effort"), serde_json::json!(5));
        patch.remove_udas.push(String::from("missing"));
        let updated =
            update(&conn, task.sid, &patch, &UrgencyConfig::default()).expect("update task");
        assert_eq!(updated.udas.get_str("effort").as_deref(), Some("5"));
        assert_eq!(updated.udas.get_str("owner").as_deref(), Some("sam"));

        let mut remove_patch = TaskPatch::default();
        remove_patch.remove_udas.push(String::from("owner"));
        let updated =
            update(&conn, task.sid, &remove_patch, &UrgencyConfig::default()).expect("remove uda");
        assert_eq!(updated.udas.get_str("effort").as_deref(), Some("5"));
        assert_eq!(updated.udas.get_str("owner"), None);
    }

    #[test]
    fn recalculates_urgency_when_tasks_change() {
        let conn = test_conn();
        let mut new_task = sample_new_task();
        new_task.priority = Some(Priority::Low);
        let task = insert(&conn, &new_task, &UrgencyConfig::default()).expect("insert task");
        let baseline_urgency = task.urgency;

        let mut patch = TaskPatch {
            priority: Some(Some(Priority::High)),
            ..TaskPatch::default()
        };
        patch.add_tags.push(String::from("next"));
        let updated = update(&conn, task.sid, &patch, &UrgencyConfig::default())
            .expect("update task urgency");

        assert!(baseline_urgency > 0.0);
        assert!(updated.urgency > baseline_urgency);
    }

    #[test]
    fn manages_dependencies_and_blocked_state() {
        let conn = test_conn();
        let dependency = insert(&conn, &sample_new_task(), &UrgencyConfig::default())
            .expect("insert dependency");
        let dependent =
            insert(&conn, &sample_new_task(), &UrgencyConfig::default()).expect("insert dependent");

        add_dependency(
            &conn,
            dependent.sid,
            dependency.sid,
            &UrgencyConfig::default(),
        )
        .expect("add dependency");
        let blocked = get_by_sid(&conn, dependent.sid)
            .expect("fetch dependent")
            .expect("dependent exists");
        assert_eq!(blocked.depends_on, vec![dependency.id]);
        assert!(is_blocked(&conn, dependent.id).expect("check blocked"));

        set_status(
            &conn,
            dependency.sid,
            TaskStatus::Done,
            &UrgencyConfig::default(),
        )
        .expect("complete dependency");
        assert!(!is_blocked(&conn, dependent.id).expect("check unblocked"));

        remove_dependency(
            &conn,
            dependent.sid,
            dependency.sid,
            &UrgencyConfig::default(),
        )
        .expect("remove dependency");
        let unblocked = get_by_sid(&conn, dependent.sid)
            .expect("fetch dependent")
            .expect("dependent exists");
        assert!(unblocked.depends_on.is_empty());
    }

    #[test]
    fn rejects_circular_dependencies() {
        let conn = test_conn();
        let task_a =
            insert(&conn, &sample_new_task(), &UrgencyConfig::default()).expect("insert task a");
        let task_b =
            insert(&conn, &sample_new_task(), &UrgencyConfig::default()).expect("insert task b");
        let task_c =
            insert(&conn, &sample_new_task(), &UrgencyConfig::default()).expect("insert task c");

        add_dependency(&conn, task_a.sid, task_b.sid, &UrgencyConfig::default())
            .expect("add first dependency");
        add_dependency(&conn, task_b.sid, task_c.sid, &UrgencyConfig::default())
            .expect("add second dependency");

        let error = add_dependency(&conn, task_c.sid, task_a.sid, &UrgencyConfig::default())
            .expect_err("reject cycle");
        assert!(matches!(error, Error::InvalidState("circular dependency")));
    }

    #[test]
    fn completing_recurring_task_creates_next_instance() {
        let conn = test_conn();
        let mut recurring = sample_new_task();
        recurring.title = String::from("Review goals");
        recurring.deadline = Some(String::from("2026-01-10"));
        recurring.recurrence = Some(r#"{"pattern":"daily","mode":"periodic"}"#.to_string());
        recurring.tags.push(String::from("ritual"));
        let original =
            insert(&conn, &recurring, &UrgencyConfig::default()).expect("insert recurring task");

        let completed = set_status(
            &conn,
            original.sid,
            TaskStatus::Done,
            &UrgencyConfig::default(),
        )
        .expect("complete task");
        assert_eq!(completed.status, TaskStatus::Done);

        let tasks = super::list(&conn, false).expect("list tasks");
        assert_eq!(tasks.len(), 2);
        let next = tasks
            .into_iter()
            .find(|task| task.sid != original.sid)
            .expect("find next instance");
        assert_eq!(next.title, "Review goals");
        assert_eq!(next.deadline.as_deref(), Some("2026-01-11"));
        assert_eq!(next.parent_id, Some(original.id));
        assert_eq!(next.recurrence, original.recurrence);
        assert_eq!(next.tags, vec![String::from("ritual")]);
    }
}
