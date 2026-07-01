//! JSON export — writes tasks as a JSON array.

use rusqlite::Connection;
use serde::Serialize;

#[derive(Serialize)]
struct AnnotationExport {
    body: String,
    created_at: String,
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
    created_at: String,
    modified_at: String,
    done_at: Option<String>,
    cancelled_at: Option<String>,
    annotations: Vec<AnnotationExport>,
}

/// Export all non-deleted tasks as a JSON array string.
///
/// # Errors
/// Returns any storage error from loading tasks or converting the export payload.
pub fn export_tasks(conn: &Connection) -> Result<String, tock_storage::Error> {
    let tasks = tock_storage::repo::task_repo::list(conn, false)?;
    let mut exports: Vec<TaskExport> = Vec::with_capacity(tasks.len());
    for task in &tasks {
        let annotations = tock_storage::repo::annotation_repo::list_for_entity(
            conn,
            task.id,
            tock_core::domain::annotation::ENTITY_KIND_TASK,
        )?
        .into_iter()
        .map(|annotation| AnnotationExport {
            body: annotation.body,
            created_at: annotation
                .created_at
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| annotation.created_at.to_string()),
        })
        .collect();

        exports.push(TaskExport {
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
            created_at: task.created_at.to_string(),
            modified_at: task.modified_at.to_string(),
            done_at: task.done_at.map(|done_at| done_at.to_string()),
            cancelled_at: task
                .cancelled_at
                .map(|cancelled_at| cancelled_at.to_string()),
            annotations,
        });
    }
    serde_json::to_string_pretty(&exports)
        .map_err(|error| tock_storage::Error::Io(std::io::Error::other(error.to_string())))
}
