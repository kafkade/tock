//! JSON export — writes tasks as a JSON array.

use rusqlite::Connection;
use serde::Serialize;

#[derive(Serialize)]
struct ChecklistItemExport {
    title: String,
    done: bool,
}

#[derive(Serialize)]
struct TaskExport {
    sid: u32,
    title: String,
    status: String,
    priority: Option<String>,
    deadline: Option<String>,
    start_date: Option<String>,
    tags: Vec<String>,
    udas: std::collections::BTreeMap<String, serde_json::Value>,
    notes: Option<String>,
    evening: bool,
    checklist: Vec<ChecklistItemExport>,
    created_at: String,
    modified_at: String,
    done_at: Option<String>,
    cancelled_at: Option<String>,
}

/// Export all non-deleted tasks as a JSON array string.
///
/// # Errors
/// Returns any storage error from loading tasks or converting the export payload.
pub fn export_tasks(conn: &Connection) -> Result<String, tock_storage::Error> {
    let tasks = tock_storage::repo::task_repo::list(conn, false)?;
    let exports: Vec<TaskExport> = tasks
        .iter()
        .map(|task| TaskExport {
            sid: task.sid,
            title: task.title.clone(),
            status: task.status.as_str().to_string(),
            priority: task
                .priority
                .as_ref()
                .map(|priority| priority.as_char().to_string()),
            deadline: task.deadline.clone(),
            start_date: task.start_date.clone(),
            tags: task.tags.clone(),
            udas: task.udas.0.clone(),
            notes: task.notes.clone(),
            evening: task.evening,
            checklist: task
                .checklist
                .iter()
                .map(|item| ChecklistItemExport {
                    title: item.title.clone(),
                    done: item.is_done(),
                })
                .collect(),
            created_at: task.created_at.to_string(),
            modified_at: task.modified_at.to_string(),
            done_at: task.done_at.map(|done_at| done_at.to_string()),
            cancelled_at: task
                .cancelled_at
                .map(|cancelled_at| cancelled_at.to_string()),
        })
        .collect();
    serde_json::to_string_pretty(&exports)
        .map_err(|error| tock_storage::Error::Io(std::io::Error::other(error.to_string())))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;

    use super::export_tasks;
    use tock_core::domain::task::{NewTask, TaskStatus};
    use tock_core::domain::urgency::UrgencyConfig;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        tock_storage::migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    #[test]
    fn exports_checklist_items() {
        let conn = test_conn();
        let task = tock_storage::repo::task_repo::insert(
            &conn,
            &NewTask {
                title: String::from("Ship release"),
                status: Some(TaskStatus::Pending),
                ..NewTask::default()
            },
            &UrgencyConfig::default(),
        )
        .expect("insert task");
        tock_storage::repo::checklist_repo::add(&conn, task.id, "cut branch").expect("add");
        tock_storage::repo::checklist_repo::set_done(&conn, task.id, 1, true).expect("check");
        tock_storage::repo::checklist_repo::add(&conn, task.id, "tag version").expect("add");

        let json = export_tasks(&conn).expect("export");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        let checklist = &parsed[0]["checklist"];
        assert_eq!(checklist[0]["title"], "cut branch");
        assert_eq!(checklist[0]["done"], true);
        assert_eq!(checklist[1]["title"], "tag version");
        assert_eq!(checklist[1]["done"], false);
    }
}
