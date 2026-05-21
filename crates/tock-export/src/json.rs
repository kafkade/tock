//! JSON export — writes tasks as a JSON array.

use rusqlite::Connection;
use serde::Serialize;

#[derive(Serialize)]
struct TaskExport {
    sid: u32,
    title: String,
    status: String,
    priority: Option<String>,
    deadline: Option<String>,
    start_date: Option<String>,
    tags: Vec<String>,
    notes: Option<String>,
    evening: bool,
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
            notes: task.notes.clone(),
            evening: task.evening,
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
