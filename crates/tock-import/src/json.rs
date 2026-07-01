//! JSON import — reads a JSON array of task objects and inserts them.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tock_core::domain::annotation::NewAnnotation;
use tock_core::domain::task::{NewTask, Priority, TaskStatus};
use tock_core::domain::uda::UdaValues;

#[derive(Deserialize)]
struct AnnotationImport {
    body: String,
    #[serde(default)]
    created_at: Option<String>,
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
    annotations: Vec<AnnotationImport>,
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
        let inserted = tock_storage::repo::task_repo::insert(
            conn,
            &new_task,
            &tock_core::domain::urgency::UrgencyConfig::default(),
        )?;

        for annotation in &import.annotations {
            let created_at = annotation
                .created_at
                .as_deref()
                .and_then(|raw| OffsetDateTime::parse(raw, &Rfc3339).ok());
            tock_storage::repo::annotation_repo::add(
                conn,
                &NewAnnotation {
                    entity_id: inserted.id,
                    entity_kind: tock_core::domain::annotation::ENTITY_KIND_TASK.to_string(),
                    body: annotation.body.clone(),
                    created_at,
                },
            )?;
        }

        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        tock_storage::migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    #[test]
    fn imports_task_with_annotations() {
        let conn = test_conn();
        let json = r#"[
            {
                "title": "Annotated task",
                "status": "pending",
                "annotations": [
                    {"body": "first", "created_at": "2026-01-01T00:00:00Z"},
                    {"body": "second"}
                ]
            }
        ]"#;

        let count = import_tasks(&conn, json).expect("import");
        assert_eq!(count, 1);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list tasks");
        assert_eq!(tasks.len(), 1);

        let annotations =
            tock_storage::repo::annotation_repo::list_for_entity(&conn, tasks[0].id, "task")
                .expect("list annotations");
        assert_eq!(annotations.len(), 2);
        assert_eq!(annotations[0].body, "first");
        assert_eq!(annotations[0].created_at.year(), 2026);
        assert_eq!(annotations[1].body, "second");
    }

    #[test]
    fn tasks_without_annotations_still_import() {
        let conn = test_conn();
        let json = r#"[{"title": "Plain task", "status": "pending"}]"#;
        let count = import_tasks(&conn, json).expect("import");
        assert_eq!(count, 1);
    }
}
