//! JSON import — reads a JSON array of task objects and inserts them.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde::Deserialize;
use tock_core::domain::task::{NewTask, Priority, TaskStatus};
use tock_core::domain::uda::UdaValues;

#[derive(Deserialize)]
struct ChecklistItemImport {
    title: String,
    #[serde(default)]
    done: bool,
}

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
    #[serde(default)]
    checklist: Vec<ChecklistItemImport>,
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
        let task = tock_storage::repo::task_repo::insert(
            conn,
            &new_task,
            &tock_core::domain::urgency::UrgencyConfig::default(),
        )?;
        for entry in &import.checklist {
            let item = tock_storage::repo::checklist_repo::add(conn, task.id, &entry.title)?;
            if entry.done {
                tock_storage::repo::checklist_repo::set_done(
                    conn,
                    task.id,
                    item.position + 1,
                    true,
                )?;
            }
        }
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;

    use super::import_tasks;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        tock_storage::migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    #[test]
    fn imports_checklist_items() {
        let conn = test_conn();
        let json = r#"[
            {
                "title": "Ship release",
                "checklist": [
                    { "title": "cut branch", "done": true },
                    { "title": "tag version", "done": false }
                ]
            }
        ]"#;
        let count = import_tasks(&conn, json).expect("import");
        assert_eq!(count, 1);

        let task = tock_storage::repo::task_repo::get_by_sid(&conn, 1)
            .expect("fetch")
            .expect("task exists");
        assert_eq!(task.checklist.len(), 2);
        assert_eq!(task.checklist[0].title, "cut branch");
        assert!(task.checklist[0].is_done());
        assert_eq!(task.checklist[1].title, "tag version");
        assert!(!task.checklist[1].is_done());
    }
}
