//! JSON import — reads a JSON array of task objects and inserts them.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde::Deserialize;
use tock_core::domain::task::{NewTask, Priority, TaskStatus};
use tock_core::domain::uda::UdaValues;

#[derive(Deserialize)]
struct TaskImport {
    title: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    deadline: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    udas: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    evening: bool,
}

/// Import tasks from a JSON string (array of task objects).
/// Returns the number of tasks imported.
///
/// # Errors
/// Returns invalid-data errors for malformed JSON and storage errors for inserts.
pub fn import_tasks(conn: &Connection, json: &str) -> Result<usize, tock_storage::Error> {
    let imports: Vec<TaskImport> = serde_json::from_str(json).map_err(|error| {
        tock_storage::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        ))
    })?;
    let mut count = 0;
    for import in &imports {
        let new_task = NewTask {
            title: import.title.clone(),
            notes: import.notes.clone(),
            status: import.status.as_deref().and_then(TaskStatus::from_str_opt),
            priority: import.priority.as_deref().and_then(Priority::from_str_opt),
            deadline: import.deadline.clone(),
            start_date: import.start_date.clone(),
            tags: import.tags.clone(),
            udas: UdaValues(import.udas.clone()),
            evening: import.evening,
            ..NewTask::default()
        };
        tock_storage::repo::task_repo::insert(
            conn,
            &new_task,
            &tock_core::domain::urgency::UrgencyConfig::default(),
        )?;
        count += 1;
    }
    Ok(count)
}
