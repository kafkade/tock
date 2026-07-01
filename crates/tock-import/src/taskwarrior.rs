//! Taskwarrior import — reads `task export` JSON and maps to tock's domain.
//!
//! Taskwarrior exports data as a JSON array of task objects via `task export`.
//! This module parses that format and creates corresponding tock tasks,
//! projects, tags, dependencies, and UDA definitions.
//!
//! ## Usage
//!
//! ```text
//! task export > tasks.json
//! tock import taskwarrior -f tasks.json
//! ```
//!
//! ## Field mapping
//!
//! | Taskwarrior        | Tock                  | Notes                                  |
//! |--------------------|-----------------------|----------------------------------------|
//! | `description`      | `title`               |                                        |
//! | `status: pending`  | `Pending`             |                                        |
//! | `status: completed`| `Done`                |                                        |
//! | `status: deleted`  | `Cancelled`           |                                        |
//! | `status: waiting`  | `Pending` + start     | `wait` → `start_date`                 |
//! | `status: recurring`| *(skipped)*           | Parent template, not a real task       |
//! | `project`          | project               | Created if absent                      |
//! | `tags`             | `tags`                |                                        |
//! | `priority` H/M/L   | `Priority`            |                                        |
//! | `due`              | `deadline`            | Extracted as `YYYY-MM-DD`              |
//! | `wait`             | `start_date`          | Extracted as `YYYY-MM-DD`              |
//! | `annotations`      | `notes`               | Concatenated with newlines             |
//! | `depends`          | dependencies          | Resolved via UUID→SID map              |
//! | `recur`            | `recurrence`          | Mapped to `RecurrenceSpec`             |
//! | UDA fields         | `udas`                | Stored as string values                |
//!
//! ## Limitations
//!
//! - `entry`, `modified`, `end` timestamps are not preserved (tock sets its own)
//! - `urgency` is recomputed by tock, not imported
//! - `imask`, `mask`, `parent` (recurring internals) are discarded
//! - Hook scripts and report aliases are not migrated

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use rusqlite::Connection;
use serde::Deserialize;
use tock_core::domain::project::NewProject;
use tock_core::domain::recurrence::{RecurrenceMode, RecurrencePattern, RecurrenceSpec};
use tock_core::domain::task::{NewTask, Priority, TaskStatus};
use tock_core::domain::uda::{UdaDefinition, UdaType, UdaValues};

/// Known Taskwarrior fields that are either mapped or intentionally ignored.
/// Anything not in this set is treated as a UDA.
const KNOWN_FIELDS: &[&str] = &[
    "id",
    "uuid",
    "description",
    "status",
    "project",
    "tags",
    "priority",
    "due",
    "wait",
    "entry",
    "modified",
    "end",
    "start",
    "scheduled",
    "depends",
    "annotations",
    "recur",
    "until",
    "urgency",
    "mask",
    "imask",
    "parent",
    "rtype",
];

/// A single annotation from Taskwarrior.
#[derive(Debug, Deserialize)]
struct TwAnnotation {
    #[serde(default)]
    description: String,
}

/// Deserialization target for a single Taskwarrior task.
///
/// Known fields are extracted explicitly; unknown fields (UDAs) are
/// captured via `#[serde(flatten)]`.
#[derive(Debug, Deserialize)]
struct TwTask {
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    due: Option<String>,
    #[serde(default)]
    wait: Option<String>,
    #[serde(default)]
    scheduled: Option<String>,
    #[serde(default)]
    depends: Option<String>,
    #[serde(default)]
    annotations: Vec<TwAnnotation>,
    #[serde(default)]
    recur: Option<String>,

    /// Catch-all for unrecognized keys (UDAs and other extras).
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

/// Summary of a Taskwarrior import operation.
#[derive(Debug, Default)]
pub struct ImportReport {
    /// Number of tasks successfully imported.
    pub tasks_imported: usize,
    /// Number of tasks skipped (recurring parent templates).
    pub tasks_skipped: usize,
    /// Names of projects that were created during import.
    pub projects_created: Vec<String>,
    /// Number of dependency edges successfully linked.
    pub dependencies_linked: usize,
    /// Number of UDA definitions registered.
    pub uda_definitions_created: usize,
    /// Warnings encountered during import (non-fatal issues).
    pub warnings: Vec<String>,
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Imported {} task(s)", self.tasks_imported)?;
        if self.tasks_skipped > 0 {
            writeln!(
                f,
                "Skipped {} recurring parent template(s)",
                self.tasks_skipped
            )?;
        }
        if !self.projects_created.is_empty() {
            writeln!(
                f,
                "Created {} project(s): {}",
                self.projects_created.len(),
                self.projects_created.join(", ")
            )?;
        }
        if self.dependencies_linked > 0 {
            writeln!(f, "Linked {} dependency edge(s)", self.dependencies_linked)?;
        }
        if self.uda_definitions_created > 0 {
            writeln!(
                f,
                "Registered {} UDA definition(s)",
                self.uda_definitions_created
            )?;
        }
        for warning in &self.warnings {
            writeln!(f, "  ⚠ {warning}")?;
        }
        Ok(())
    }
}

/// Import tasks from a Taskwarrior JSON export (`task export` output).
///
/// The import runs inside a transaction — either everything succeeds or
/// nothing is committed.
///
/// # Errors
///
/// Returns an error if the JSON is malformed or a storage operation fails.
/// Individual warnings (missing deps, unsupported fields) are collected in
/// the report rather than failing the import.
pub fn import_taskwarrior(
    conn: &mut Connection,
    json: &str,
) -> Result<ImportReport, tock_storage::Error> {
    let tasks: Vec<TwTask> = serde_json::from_str(json).map_err(|error| {
        tock_storage::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        ))
    })?;

    let tx = conn.transaction()?;
    let report = import_within_transaction(&tx, &tasks)?;
    tx.commit()?;

    Ok(report)
}

/// Core import logic, operating within an existing transaction.
fn import_within_transaction(
    conn: &Connection,
    tasks: &[TwTask],
) -> Result<ImportReport, tock_storage::Error> {
    let mut report = ImportReport::default();

    // Load existing projects for dedup.
    let existing_projects = tock_storage::repo::project_repo::list(conn, true)?;
    let mut project_cache: HashMap<String, uuid::Uuid> = existing_projects
        .into_iter()
        .map(|p| (p.name.clone(), p.id))
        .collect();

    // Load existing UDA definitions to avoid duplicates.
    let existing_udas = tock_storage::repo::uda_repo::list_definitions(conn)?;
    let mut known_uda_keys: HashSet<String> = existing_udas.into_iter().map(|d| d.key).collect();

    // Pass 1: insert tasks, build UUID→SID map.
    let mut uuid_to_sid: HashMap<String, u32> = HashMap::new();
    let mut deferred_deps: Vec<(String, Vec<String>)> = Vec::new();

    for tw in tasks {
        let status_str = tw.status.as_deref().unwrap_or("pending");

        if status_str == "recurring" {
            report.tasks_skipped += 1;
            continue;
        }

        let (new_task, tw_uuid, deps) = convert_task(
            tw,
            status_str,
            conn,
            &mut project_cache,
            &mut known_uda_keys,
            &mut report,
        )?;

        let inserted = tock_storage::repo::task_repo::insert(
            conn,
            &new_task,
            &tock_core::domain::urgency::UrgencyConfig::default(),
        )?;

        if !tw_uuid.is_empty() {
            uuid_to_sid.insert(tw_uuid.clone(), inserted.sid);
        }

        if !deps.is_empty() {
            deferred_deps.push((tw_uuid, deps));
        }

        report.tasks_imported += 1;
    }

    // Pass 2: wire up dependencies.
    link_dependencies(conn, &deferred_deps, &uuid_to_sid, &mut report)?;

    Ok(report)
}

/// Convert a single Taskwarrior task into a [`NewTask`], returning the
/// Taskwarrior UUID and any dependency UUIDs for deferred linking.
fn convert_task(
    tw: &TwTask,
    status_str: &str,
    conn: &Connection,
    project_cache: &mut HashMap<String, uuid::Uuid>,
    known_uda_keys: &mut HashSet<String>,
    report: &mut ImportReport,
) -> Result<(NewTask, String, Vec<String>), tock_storage::Error> {
    let description = tw
        .description
        .as_deref()
        .unwrap_or("(untitled)")
        .to_string();

    let tw_uuid = tw.uuid.clone().unwrap_or_default();

    let tock_status = match status_str {
        "pending" | "waiting" => Some(TaskStatus::Pending),
        "completed" => Some(TaskStatus::Done),
        "deleted" => Some(TaskStatus::Cancelled),
        other => {
            report.warnings.push(format!(
                "Unknown status '{other}' for task '{description}', defaulting to Pending"
            ));
            Some(TaskStatus::Pending)
        }
    };

    let priority = tw.priority.as_deref().and_then(Priority::from_str_opt);

    let deadline = tw.due.as_deref().and_then(|d| {
        parse_tw_date(d).or_else(|| {
            report
                .warnings
                .push(format!("Invalid due date '{d}' for task '{description}'"));
            None
        })
    });

    let start_date = if status_str == "waiting" {
        tw.wait.as_deref().and_then(|d| {
            parse_tw_date(d).or_else(|| {
                report
                    .warnings
                    .push(format!("Invalid wait date '{d}' for task '{description}'"));
                None
            })
        })
    } else {
        None
    };

    let notes = if tw.annotations.is_empty() {
        None
    } else {
        Some(
            tw.annotations
                .iter()
                .map(|a| a.description.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };

    let project_id = resolve_project(conn, tw.project.as_deref(), project_cache, report)?;

    let recurrence = tw.recur.as_deref().and_then(|r| {
        parse_tw_recurrence(r)
            .map(|spec| spec.to_json())
            .or_else(|| {
                report.warnings.push(format!(
                    "Unsupported recurrence '{r}' for task '{description}'"
                ));
                None
            })
    });

    if tw.scheduled.is_some() {
        report.warnings.push(format!(
            "Scheduled date not preserved for task '{description}'"
        ));
    }

    let udas = collect_udas(conn, &tw.extra, known_uda_keys, report)?;

    let mut tags = tw.tags.clone();
    if status_str == "waiting" && !tags.iter().any(|t| t == "waiting") {
        tags.push(String::from("waiting"));
    }

    let deps = parse_depends_field(tw.depends.as_deref());

    let new_task = NewTask {
        title: description,
        notes,
        status: tock_status,
        project_id,
        start_date,
        deadline,
        recurrence,
        priority,
        tags,
        udas: UdaValues(udas),
        ..NewTask::default()
    };

    Ok((new_task, tw_uuid, deps))
}

/// Collect UDA fields from the task's extra map, registering definitions.
fn collect_udas(
    conn: &Connection,
    extra: &BTreeMap<String, serde_json::Value>,
    known_uda_keys: &mut HashSet<String>,
    report: &mut ImportReport,
) -> Result<BTreeMap<String, serde_json::Value>, tock_storage::Error> {
    let mut udas = BTreeMap::new();
    for (key, value) in extra {
        if KNOWN_FIELDS.contains(&key.as_str()) {
            continue;
        }
        udas.insert(key.clone(), value.clone());
        if known_uda_keys.insert(key.clone()) {
            let def = UdaDefinition {
                key: key.clone(),
                uda_type: UdaType::String,
                label: Some(key.clone()),
                default: None,
            };
            tock_storage::repo::uda_repo::add_definition(conn, &def)?;
            report.uda_definitions_created += 1;
        }
    }
    Ok(udas)
}

/// Parse the Taskwarrior `depends` field (comma-separated UUIDs).
fn parse_depends_field(raw: Option<&str>) -> Vec<String> {
    raw.map(|s| {
        s.split(',')
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

/// Find or create a project by name, caching results for dedup.
fn resolve_project(
    conn: &Connection,
    name: Option<&str>,
    cache: &mut HashMap<String, uuid::Uuid>,
    report: &mut ImportReport,
) -> Result<Option<uuid::Uuid>, tock_storage::Error> {
    let Some(raw_name) = name else {
        return Ok(None);
    };
    let trimmed = raw_name.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if let Some(&id) = cache.get(trimmed) {
        return Ok(Some(id));
    }
    let new_project = NewProject {
        name: trimmed.to_string(),
        notes: None,
        area_id: None,
        deadline: None,
    };
    let project = tock_storage::repo::project_repo::insert(conn, &new_project)?;
    cache.insert(trimmed.to_string(), project.id);
    report.projects_created.push(trimmed.to_string());
    Ok(Some(project.id))
}

/// Wire up dependency edges from the deferred list, warning on failures.
fn link_dependencies(
    conn: &Connection,
    deferred: &[(String, Vec<String>)],
    uuid_to_sid: &HashMap<String, u32>,
    report: &mut ImportReport,
) -> Result<(), tock_storage::Error> {
    for (task_uuid, dep_uuids) in deferred {
        let Some(&task_sid) = uuid_to_sid.get(task_uuid.as_str()) else {
            report.warnings.push(format!(
                "Could not resolve task UUID '{task_uuid}' for dependency linking"
            ));
            continue;
        };
        for dep_uuid in dep_uuids {
            let Some(&dep_sid) = uuid_to_sid.get(dep_uuid.as_str()) else {
                report.warnings.push(format!(
                    "Dependency target UUID '{dep_uuid}' not found in import set"
                ));
                continue;
            };

            match tock_storage::repo::task_repo::add_dependency(
                conn,
                task_sid,
                dep_sid,
                &tock_core::domain::urgency::UrgencyConfig::default(),
            ) {
                Ok(()) => report.dependencies_linked += 1,
                Err(tock_storage::Error::InvalidState(reason)) => {
                    report.warnings.push(format!(
                        "Could not link dependency {task_uuid} → {dep_uuid}: {reason}"
                    ));
                }
                Err(e) => return Err(e),
            }
        }
    }

    Ok(())
}

/// Parse a Taskwarrior timestamp (`YYYYMMDDTHHMMSSz` or `YYYYMMDD`) into
/// `YYYY-MM-DD` format. Returns `None` if the format is invalid.
fn parse_tw_date(raw: &str) -> Option<String> {
    // Taskwarrior uses `20260115T120000Z` format.
    // We only need the date portion (first 8 chars: YYYYMMDD).
    let date_part = if raw.len() >= 8 {
        &raw[..8]
    } else {
        return None;
    };

    // Validate that the 8-char prefix is all digits.
    if !date_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let year: u32 = date_part[..4].parse().ok()?;
    let month: u32 = date_part[4..6].parse().ok()?;
    let day: u32 = date_part[6..8].parse().ok()?;

    // Basic validity checks.
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || year == 0 {
        return None;
    }

    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// Parse a Taskwarrior recurrence string into a [`RecurrenceSpec`].
///
/// Taskwarrior supports: `daily`, `weekly`, `monthly`, `yearly`,
/// `<N>d` (every N days), `<N>w` (every N weeks).
fn parse_tw_recurrence(raw: &str) -> Option<RecurrenceSpec> {
    let trimmed = raw.trim().to_lowercase();

    let pattern = match trimmed.as_str() {
        "daily" => RecurrencePattern::Daily,
        "weekly" | "weekdays" => RecurrencePattern::Weekly,
        "monthly" => RecurrencePattern::Monthly,
        "yearly" | "annual" => RecurrencePattern::Yearly,
        other => {
            // Try patterns like "3d", "2w".
            if let Some(num_str) = other.strip_suffix('d') {
                let n: u32 = num_str.parse().ok()?;
                if n == 0 {
                    return None;
                }
                RecurrencePattern::EveryNDays(n)
            } else if let Some(num_str) = other.strip_suffix('w') {
                let n: u32 = num_str.parse().ok()?;
                if n == 0 {
                    return None;
                }
                RecurrencePattern::EveryNWeeks(n)
            } else {
                return None;
            }
        }
    };

    Some(RecurrenceSpec {
        pattern,
        mode: RecurrenceMode::Periodic,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;
    use tock_core::domain::task::TaskStatus;

    use super::{import_taskwarrior, parse_tw_date, parse_tw_recurrence};
    use tock_core::domain::recurrence::{RecurrenceMode, RecurrencePattern};

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        tock_storage::migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    // ── Date parsing ──────────────────────────────────────────────

    #[test]
    fn parses_taskwarrior_timestamp() {
        assert_eq!(
            parse_tw_date("20260115T120000Z"),
            Some(String::from("2026-01-15"))
        );
    }

    #[test]
    fn parses_date_only() {
        assert_eq!(parse_tw_date("20260115"), Some(String::from("2026-01-15")));
    }

    #[test]
    fn rejects_short_date() {
        assert_eq!(parse_tw_date("2026"), None);
    }

    #[test]
    fn rejects_invalid_month() {
        assert_eq!(parse_tw_date("20261315T000000Z"), None);
    }

    #[test]
    fn rejects_non_numeric_date() {
        assert_eq!(parse_tw_date("abcdefgh"), None);
    }

    // ── Recurrence parsing ────────────────────────────────────────

    #[test]
    fn parses_daily_recurrence() {
        let spec = parse_tw_recurrence("daily").expect("parse daily");
        assert_eq!(spec.pattern, RecurrencePattern::Daily);
        assert_eq!(spec.mode, RecurrenceMode::Periodic);
    }

    #[test]
    fn parses_weekly_recurrence() {
        let spec = parse_tw_recurrence("weekly").expect("parse weekly");
        assert_eq!(spec.pattern, RecurrencePattern::Weekly);
    }

    #[test]
    fn parses_every_n_days() {
        let spec = parse_tw_recurrence("3d").expect("parse 3d");
        assert_eq!(spec.pattern, RecurrencePattern::EveryNDays(3));
    }

    #[test]
    fn parses_every_n_weeks() {
        let spec = parse_tw_recurrence("2w").expect("parse 2w");
        assert_eq!(spec.pattern, RecurrencePattern::EveryNWeeks(2));
    }

    #[test]
    fn rejects_zero_interval() {
        assert!(parse_tw_recurrence("0d").is_none());
    }

    #[test]
    fn rejects_unknown_recurrence() {
        assert!(parse_tw_recurrence("biweekly").is_none());
    }

    // ── Full import ───────────────────────────────────────────────

    #[test]
    fn imports_basic_task() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                "description": "Buy groceries",
                "status": "pending",
                "tags": ["shopping", "personal"],
                "priority": "M",
                "entry": "20260101T000000Z"
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 1);
        assert_eq!(report.tasks_skipped, 0);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Buy groceries");
        assert_eq!(tasks[0].status, TaskStatus::Pending);
        assert!(tasks[0].tags.contains(&String::from("shopping")));
        assert!(tasks[0].tags.contains(&String::from("personal")));
        assert_eq!(
            tasks[0].priority,
            Some(tock_core::domain::task::Priority::Medium)
        );
    }

    #[test]
    fn imports_completed_task() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "11111111-1111-1111-1111-111111111111",
                "description": "Completed thing",
                "status": "completed",
                "end": "20260110T150000Z"
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 1);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list tasks");
        assert_eq!(tasks[0].status, TaskStatus::Done);
    }

    #[test]
    fn imports_deleted_task_as_cancelled() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "22222222-2222-2222-2222-222222222222",
                "description": "Deleted thing",
                "status": "deleted"
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 1);

        let tasks = tock_storage::repo::task_repo::list(&conn, true).expect("list tasks");
        assert_eq!(tasks[0].status, TaskStatus::Cancelled);
    }

    #[test]
    fn imports_waiting_task_with_start_date() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "33333333-3333-3333-3333-333333333333",
                "description": "Future task",
                "status": "waiting",
                "wait": "20260601T000000Z"
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 1);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list tasks");
        assert_eq!(tasks[0].status, TaskStatus::Pending);
        assert_eq!(tasks[0].start_date.as_deref(), Some("2026-06-01"));
        assert!(tasks[0].tags.contains(&String::from("waiting")));
    }

    #[test]
    fn skips_recurring_parent_templates() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "44444444-4444-4444-4444-444444444444",
                "description": "Recurring template",
                "status": "recurring",
                "recur": "weekly"
            },
            {
                "uuid": "55555555-5555-5555-5555-555555555555",
                "description": "Normal task",
                "status": "pending"
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 1);
        assert_eq!(report.tasks_skipped, 1);
    }

    #[test]
    fn creates_project_for_task() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "66666666-6666-6666-6666-666666666666",
                "description": "Project task",
                "status": "pending",
                "project": "home.garden"
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 1);
        assert_eq!(report.projects_created, vec!["home.garden"]);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list tasks");
        assert!(tasks[0].project_id.is_some());
    }

    #[test]
    fn deduplicates_projects() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "77777777-7777-7777-7777-777777777777",
                "description": "Task A",
                "status": "pending",
                "project": "work"
            },
            {
                "uuid": "88888888-8888-8888-8888-888888888888",
                "description": "Task B",
                "status": "pending",
                "project": "work"
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 2);
        assert_eq!(report.projects_created.len(), 1);

        let projects = tock_storage::repo::project_repo::list(&conn, false).expect("list projects");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "work");
    }

    #[test]
    fn imports_due_date() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "99999999-9999-9999-9999-999999999999",
                "description": "Deadline task",
                "status": "pending",
                "due": "20260315T170000Z"
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 1);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list tasks");
        assert_eq!(tasks[0].deadline.as_deref(), Some("2026-03-15"));
    }

    #[test]
    fn imports_annotations_as_notes() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "description": "Annotated task",
                "status": "pending",
                "annotations": [
                    {"entry": "20260101T000000Z", "description": "First note"},
                    {"entry": "20260102T000000Z", "description": "Second note"}
                ]
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 1);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list tasks");
        assert_eq!(tasks[0].notes.as_deref(), Some("First note\nSecond note"));
    }

    #[test]
    fn imports_dependencies() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                "description": "Dependency",
                "status": "pending"
            },
            {
                "uuid": "cccccccc-cccc-cccc-cccc-cccccccccccc",
                "description": "Dependent",
                "status": "pending",
                "depends": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 2);
        assert_eq!(report.dependencies_linked, 1);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list tasks");
        let dependent = tasks
            .iter()
            .find(|t| t.title == "Dependent")
            .expect("find dependent");
        assert!(!dependent.depends_on.is_empty());
    }

    #[test]
    fn warns_on_missing_dependency_target() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "dddddddd-dddd-dddd-dddd-dddddddddddd",
                "description": "Orphaned dep",
                "status": "pending",
                "depends": "ffffffff-ffff-ffff-ffff-ffffffffffff"
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 1);
        assert_eq!(report.dependencies_linked, 0);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("not found in import set"))
        );
    }

    #[test]
    fn imports_recurrence() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee",
                "description": "Recurring child",
                "status": "pending",
                "recur": "daily",
                "due": "20260201T000000Z"
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 1);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list tasks");
        assert!(tasks[0].recurrence.is_some());
    }

    #[test]
    fn imports_udas() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "f0f0f0f0-f0f0-f0f0-f0f0-f0f0f0f0f0f0",
                "description": "Task with UDA",
                "status": "pending",
                "effort": "high",
                "estimate": 3
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 1);
        assert_eq!(report.uda_definitions_created, 2);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list tasks");
        assert_eq!(tasks[0].udas.get_str("effort"), Some(String::from("high")));

        let defs = tock_storage::repo::uda_repo::list_definitions(&conn).expect("list uda defs");
        assert!(defs.iter().any(|d| d.key == "effort"));
        assert!(defs.iter().any(|d| d.key == "estimate"));
    }

    #[test]
    fn handles_empty_export() {
        let mut conn = test_conn();
        let report = import_taskwarrior(&mut conn, "[]").expect("import empty");
        assert_eq!(report.tasks_imported, 0);
        assert_eq!(report.tasks_skipped, 0);
    }

    #[test]
    fn rejects_malformed_json() {
        let mut conn = test_conn();
        assert!(import_taskwarrior(&mut conn, "not json").is_err());
    }

    #[test]
    fn imports_multiple_dependencies() {
        let mut conn = test_conn();
        let json = r#"[
            {
                "uuid": "11111111-aaaa-bbbb-cccc-111111111111",
                "description": "Dep A",
                "status": "pending"
            },
            {
                "uuid": "22222222-aaaa-bbbb-cccc-222222222222",
                "description": "Dep B",
                "status": "pending"
            },
            {
                "uuid": "33333333-aaaa-bbbb-cccc-333333333333",
                "description": "Depends on both",
                "status": "pending",
                "depends": "11111111-aaaa-bbbb-cccc-111111111111,22222222-aaaa-bbbb-cccc-222222222222"
            }
        ]"#;

        let report = import_taskwarrior(&mut conn, json).expect("import");
        assert_eq!(report.tasks_imported, 3);
        assert_eq!(report.dependencies_linked, 2);
    }
}
