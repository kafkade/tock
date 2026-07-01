//! Things 3 import — reads a portable canonical JSON export and maps it to
//! tock's domain (areas, projects, headings, tasks).
//!
//! Things 3 has no official export. §9.3 of the architecture describes two
//! source paths: an `AppleScript` walk that emits our canonical JSON, and a
//! manual JSON file (the JSON URL-scheme dump or a third-party tool such as
//! `things-cli export`). This module implements the **JSON** path: it accepts
//! the canonical JSON and creates the corresponding tock entities inside a
//! single transaction.
//!
//! ## Canonical JSON shape
//!
//! The export mirrors the Areas → Projects → Headings → To-Dos walk:
//!
//! ```jsonc
//! {
//!   "areas": [
//!     {
//!       "uuid": "area-work",
//!       "title": "Work",
//!       "tags": ["focus"],
//!       "projects": [ /* Project */ ],
//!       "todos":    [ /* area-level To-Do with no project */ ]
//!     }
//!   ],
//!   "projects": [ /* Projects with no area */ ],
//!   "todos":    [ /* loose inbox To-Dos */ ]
//! }
//! ```
//!
//! A Project holds `headings` and `todos`; a To-Do references a heading by
//! `uuid`, carries `when` / `deadline` / `tags` / `notes` / `status`, an
//! optional `repeat` rule, and `checklist` items.
//!
//! ## Field mapping (§9.3)
//!
//! | Things 3            | tock                                            |
//! |---------------------|-------------------------------------------------|
//! | `title`             | `title`                                         |
//! | `notes`             | `notes` (checklist appended below)              |
//! | `when` = today      | `start_date` = today                            |
//! | `when` = evening    | `start_date` = today, `evening = true`          |
//! | `when` = someday    | status `Someday` + `+someday` tag               |
//! | `when` = `<date>`   | `start_date`                                    |
//! | `deadline`          | `deadline` (due date)                           |
//! | `tags`              | `tags` (kept flat, `/` preserved)               |
//! | `checklistItems`    | appended to `notes` as `- [ ] / - [x]` lines    |
//! | `status` open       | `Pending`                                        |
//! | `status` completed  | `Done`                                          |
//! | `status` canceled   | `Cancelled`                                     |
//! | `repeat`            | `recurrence` (`RecurrenceSpec` JSON)              |
//! | Area                | Area (created / deduped by name)                |
//! | Project             | Project (area inherited)                        |
//! | Heading             | Heading on project; task `heading_id` set       |
//!
//! ## Limitations (surfaced in the report)
//!
//! - Checklist items degrade into the task's `notes` (no checklist domain yet).
//! - Trashed items are skipped unless `--include-trash` is passed.
//! - Non-open project statuses and unsupported `repeat` rules produce warnings.

use std::collections::HashMap;
use std::fmt;
use std::fmt::Write as _;

use rusqlite::Connection;
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

use tock_core::domain::area::NewArea;
use tock_core::domain::heading::NewHeading;
use tock_core::domain::project::NewProject;
use tock_core::domain::recurrence::{RecurrenceMode, RecurrencePattern, RecurrenceSpec};
use tock_core::domain::task::{NewTask, TaskStatus};

// ── Deserialization types ─────────────────────────────────────────

/// Top-level canonical Things 3 export.
#[derive(Debug, Default, Deserialize)]
struct Things3Export {
    #[serde(default)]
    areas: Vec<Area>,
    #[serde(default)]
    projects: Vec<Project>,
    #[serde(default, alias = "tasks", alias = "items")]
    todos: Vec<Todo>,
}

/// A Things 3 area (top-level grouping).
#[derive(Debug, Default, Deserialize)]
struct Area {
    #[serde(default, alias = "name")]
    title: String,
    #[serde(default)]
    projects: Vec<Project>,
    #[serde(default, alias = "tasks", alias = "items")]
    todos: Vec<Todo>,
}

/// A Things 3 project.
#[derive(Debug, Default, Deserialize)]
struct Project {
    #[serde(default, alias = "name")]
    title: String,
    #[serde(default, alias = "note")]
    notes: Option<String>,
    #[serde(default, alias = "due")]
    deadline: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    headings: Vec<Heading>,
    #[serde(default, alias = "tasks", alias = "items")]
    todos: Vec<Todo>,
    #[serde(default)]
    trashed: bool,
}

/// A heading within a project.
#[derive(Debug, Default, Deserialize)]
struct Heading {
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default, alias = "name")]
    title: String,
}

/// A Things 3 to-do (task).
#[derive(Debug, Default, Deserialize)]
struct Todo {
    #[serde(default, alias = "name")]
    title: String,
    #[serde(default, alias = "note")]
    notes: Option<String>,
    #[serde(default)]
    when: Option<String>,
    #[serde(default, alias = "due")]
    deadline: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    heading: Option<String>,
    #[serde(default, alias = "checklistItems")]
    checklist: Vec<ChecklistItem>,
    #[serde(default)]
    repeat: Option<String>,
    #[serde(default)]
    trashed: bool,
}

/// A single checklist item (degraded into notes on import).
#[derive(Debug, Default, Deserialize)]
struct ChecklistItem {
    #[serde(default, alias = "name")]
    title: String,
    #[serde(default, alias = "done")]
    completed: bool,
}

// ── Report ────────────────────────────────────────────────────────

/// Summary of a Things 3 import operation.
#[derive(Debug, Default)]
pub struct ImportReport {
    /// Names of areas created during import.
    pub areas_created: Vec<String>,
    /// Names of projects created during import.
    pub projects_created: Vec<String>,
    /// Number of headings created.
    pub headings_created: usize,
    /// Number of tasks successfully imported.
    pub tasks_imported: usize,
    /// Number of checklist items degraded into task notes.
    pub checklist_items_degraded: usize,
    /// Number of trashed items skipped (projects + to-dos).
    pub trashed_skipped: usize,
    /// Warnings encountered during import (non-fatal issues).
    pub warnings: Vec<String>,
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Imported {} task(s)", self.tasks_imported)?;
        if !self.areas_created.is_empty() {
            writeln!(
                f,
                "Created {} area(s): {}",
                self.areas_created.len(),
                self.areas_created.join(", ")
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
        if self.headings_created > 0 {
            writeln!(f, "Created {} heading(s)", self.headings_created)?;
        }
        if self.checklist_items_degraded > 0 {
            writeln!(
                f,
                "Degraded {} checklist item(s) into task notes",
                self.checklist_items_degraded
            )?;
        }
        if self.trashed_skipped > 0 {
            writeln!(f, "Skipped {} trashed item(s)", self.trashed_skipped)?;
        }
        for warning in &self.warnings {
            writeln!(f, "  ⚠ {warning}")?;
        }
        Ok(())
    }
}

// ── Import entry point ────────────────────────────────────────────

/// Import areas, projects, headings, and tasks from a Things 3 canonical
/// JSON export.
///
/// The import runs inside a single transaction — either everything succeeds
/// or nothing is committed. Trashed items are skipped unless `include_trash`
/// is `true`.
///
/// # Errors
///
/// Returns an error if the JSON is malformed or a storage operation fails.
/// Individual non-fatal issues (invalid dates, unsupported recurrence,
/// checklist degradation) are collected in the report rather than failing.
pub fn import_things3(
    conn: &mut Connection,
    json: &str,
    include_trash: bool,
) -> Result<ImportReport, tock_storage::Error> {
    let export: Things3Export = serde_json::from_str(json).map_err(|error| {
        tock_storage::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        ))
    })?;

    let today = today_string();
    let tx = conn.transaction()?;
    let report = import_within_transaction(&tx, &export, &today, include_trash)?;
    tx.commit()?;

    Ok(report)
}

/// Caches for deduping areas and projects by name across the import.
struct Caches {
    areas: HashMap<String, Uuid>,
    projects: HashMap<String, Uuid>,
}

/// Core import logic, operating within an existing transaction.
fn import_within_transaction(
    conn: &Connection,
    export: &Things3Export,
    today: &str,
    include_trash: bool,
) -> Result<ImportReport, tock_storage::Error> {
    let mut report = ImportReport::default();

    let areas = tock_storage::repo::area_repo::list(conn, true)?;
    let projects = tock_storage::repo::project_repo::list(conn, true)?;
    let mut caches = Caches {
        areas: areas.into_iter().map(|a| (a.name, a.id)).collect(),
        projects: projects.into_iter().map(|p| (p.name, p.id)).collect(),
    };

    // Areas and everything nested under them.
    for area in &export.areas {
        let area_id = resolve_area(conn, &area.title, &mut caches, &mut report)?;
        for project in &area.projects {
            import_project(
                conn,
                project,
                area_id,
                &mut caches,
                &mut report,
                today,
                include_trash,
            )?;
        }
        for todo in &area.todos {
            import_task(
                conn,
                todo,
                None,
                area_id,
                &HashMap::new(),
                &mut report,
                today,
                include_trash,
            )?;
        }
    }

    // Projects with no area.
    for project in &export.projects {
        import_project(
            conn,
            project,
            None,
            &mut caches,
            &mut report,
            today,
            include_trash,
        )?;
    }

    // Loose inbox to-dos.
    for todo in &export.todos {
        import_task(
            conn,
            todo,
            None,
            None,
            &HashMap::new(),
            &mut report,
            today,
            include_trash,
        )?;
    }

    Ok(report)
}

/// Import a project plus its headings and to-dos.
fn import_project(
    conn: &Connection,
    project: &Project,
    area_id: Option<Uuid>,
    caches: &mut Caches,
    report: &mut ImportReport,
    today: &str,
    include_trash: bool,
) -> Result<(), tock_storage::Error> {
    if project.trashed && !include_trash {
        // Count the project itself plus every to-do lost with it.
        report.trashed_skipped += 1 + project.todos.len();
        return Ok(());
    }

    let name = project.title.trim();
    if name.is_empty() {
        report
            .warnings
            .push(String::from("Skipped a project with an empty title"));
        return Ok(());
    }

    if let Some(status) = project.status.as_deref() {
        let normalized = status.trim().to_lowercase();
        if !matches!(normalized.as_str(), "" | "open" | "active") {
            report.warnings.push(format!(
                "Project '{name}' status '{status}' not preserved (projects have no status field)"
            ));
        }
    }

    let project_id = resolve_project(
        conn,
        name,
        project.notes.clone(),
        area_id,
        project.deadline.as_deref(),
        caches,
        report,
    )?;

    // Create headings and build a lookup from Things heading uuid/title to the
    // new tock heading id.
    let mut heading_map: HashMap<String, Uuid> = HashMap::new();
    for heading in &project.headings {
        let text = heading.title.trim();
        if text.is_empty() {
            continue;
        }
        let inserted = tock_storage::repo::heading_repo::insert(
            conn,
            &NewHeading {
                project_id,
                name: text.to_string(),
            },
        )?;
        report.headings_created += 1;
        if let Some(uuid) = heading.uuid.as_deref() {
            heading_map.insert(uuid.to_string(), inserted.id);
        }
        heading_map.insert(text.to_string(), inserted.id);
    }

    for todo in &project.todos {
        import_task(
            conn,
            todo,
            Some(project_id),
            None,
            &heading_map,
            report,
            today,
            include_trash,
        )?;
    }

    Ok(())
}

/// Import a single to-do into a task.
#[allow(clippy::too_many_arguments)]
fn import_task(
    conn: &Connection,
    todo: &Todo,
    project_id: Option<Uuid>,
    area_id: Option<Uuid>,
    heading_map: &HashMap<String, Uuid>,
    report: &mut ImportReport,
    today: &str,
    include_trash: bool,
) -> Result<(), tock_storage::Error> {
    if todo.trashed && !include_trash {
        report.trashed_skipped += 1;
        return Ok(());
    }

    let heading_id = todo
        .heading
        .as_deref()
        .and_then(|h| heading_map.get(h).copied());

    if todo.heading.is_some() && heading_id.is_none() {
        report.warnings.push(format!(
            "Heading '{}' not found for task '{}'",
            todo.heading.as_deref().unwrap_or_default(),
            todo.title.trim()
        ));
    }

    let new_task = convert_todo(todo, project_id, area_id, heading_id, today, report);

    tock_storage::repo::task_repo::insert(
        conn,
        &new_task,
        &tock_core::domain::urgency::UrgencyConfig::default(),
    )?;
    report.tasks_imported += 1;

    Ok(())
}

/// Convert a Things 3 to-do into a [`NewTask`], collecting warnings.
fn convert_todo(
    todo: &Todo,
    project_id: Option<Uuid>,
    area_id: Option<Uuid>,
    heading_id: Option<Uuid>,
    today: &str,
    report: &mut ImportReport,
) -> NewTask {
    let title = todo.title.trim().to_string();
    if title.is_empty() {
        report
            .warnings
            .push(String::from("Imported a task with an empty title"));
    }

    let mut status = map_status(todo.status.as_deref());
    let mut tags = todo.tags.clone();

    // "when" is Things' start (defer), not a deadline.
    let mut start_date = None;
    let mut evening = false;
    match todo.when.as_deref().map(str::trim) {
        None | Some("" | "anytime") => {}
        Some(w) if w.eq_ignore_ascii_case("today") => start_date = Some(today.to_string()),
        Some(w) if w.eq_ignore_ascii_case("evening") => {
            start_date = Some(today.to_string());
            evening = true;
        }
        Some(w) if w.eq_ignore_ascii_case("someday") => {
            // Only downgrade an otherwise-pending task to Someday.
            if status == TaskStatus::Pending {
                status = TaskStatus::Someday;
            }
            if !tags.iter().any(|t| t == "someday") {
                tags.push(String::from("someday"));
            }
        }
        Some(other) => match parse_things_date(other) {
            Some(date) => start_date = Some(date),
            None => report
                .warnings
                .push(format!("Invalid 'when' date '{other}' for task '{title}'")),
        },
    }

    let deadline = todo.deadline.as_deref().and_then(|d| {
        parse_things_date(d).or_else(|| {
            report
                .warnings
                .push(format!("Invalid deadline '{d}' for task '{title}'"));
            None
        })
    });

    let notes = build_notes(todo, report);

    let recurrence = todo.repeat.as_deref().and_then(|r| {
        parse_repeat(r).map(|spec| spec.to_json()).or_else(|| {
            report
                .warnings
                .push(format!("Unsupported repeat rule '{r}' for task '{title}'"));
            None
        })
    });

    NewTask {
        title,
        notes,
        status: Some(status),
        project_id,
        area_id,
        heading_id,
        start_date,
        deadline,
        recurrence,
        evening,
        tags,
        ..NewTask::default()
    }
}

/// Build the task notes, appending checklist items as checkbox lines.
fn build_notes(todo: &Todo, report: &mut ImportReport) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(notes) = todo.notes.as_deref() {
        let trimmed = notes.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }

    let checklist: Vec<&ChecklistItem> = todo
        .checklist
        .iter()
        .filter(|item| !item.title.trim().is_empty())
        .collect();

    if !checklist.is_empty() {
        let mut lines = String::from("Checklist:");
        for item in &checklist {
            let mark = if item.completed { 'x' } else { ' ' };
            let _ = write!(lines, "\n- [{mark}] {}", item.title.trim());
        }
        report.checklist_items_degraded += checklist.len();
        parts.push(lines);
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

/// Map a Things 3 status string to a tock [`TaskStatus`].
fn map_status(status: Option<&str>) -> TaskStatus {
    match status.map(|s| s.trim().to_lowercase()).as_deref() {
        Some("completed" | "complete" | "done") => TaskStatus::Done,
        Some("canceled" | "cancelled") => TaskStatus::Cancelled,
        _ => TaskStatus::Pending,
    }
}

/// Find or create an area by name, caching results for dedup.
fn resolve_area(
    conn: &Connection,
    title: &str,
    caches: &mut Caches,
    report: &mut ImportReport,
) -> Result<Option<Uuid>, tock_storage::Error> {
    let name = title.trim();
    if name.is_empty() {
        return Ok(None);
    }
    if let Some(&id) = caches.areas.get(name) {
        return Ok(Some(id));
    }
    let area = tock_storage::repo::area_repo::insert(
        conn,
        &NewArea {
            name: name.to_string(),
            color: None,
        },
    )?;
    caches.areas.insert(name.to_string(), area.id);
    report.areas_created.push(name.to_string());
    Ok(Some(area.id))
}

/// Find or create a project by name, caching results for dedup.
fn resolve_project(
    conn: &Connection,
    name: &str,
    notes: Option<String>,
    area_id: Option<Uuid>,
    deadline: Option<&str>,
    caches: &mut Caches,
    report: &mut ImportReport,
) -> Result<Uuid, tock_storage::Error> {
    if let Some(&id) = caches.projects.get(name) {
        return Ok(id);
    }
    let deadline = deadline.and_then(|d| {
        parse_things_date(d).or_else(|| {
            report
                .warnings
                .push(format!("Invalid deadline '{d}' for project '{name}'"));
            None
        })
    });
    let project = tock_storage::repo::project_repo::insert(
        conn,
        &NewProject {
            name: name.to_string(),
            notes,
            area_id,
            deadline,
        },
    )?;
    caches.projects.insert(name.to_string(), project.id);
    report.projects_created.push(name.to_string());
    Ok(project.id)
}

// ── Parsing helpers ───────────────────────────────────────────────

/// Parse a Things 3 date into `YYYY-MM-DD`. Accepts `YYYY-MM-DD`,
/// `YYYY-MM-DDTHH:MM:SS`, and `YYYYMMDD`. Returns `None` if invalid.
fn parse_things_date(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    // Drop any time component.
    let date_part = trimmed.split(['T', ' ']).next().unwrap_or(trimmed);

    let (year, month, day) = if let Some((y, rest)) = date_part.split_once('-') {
        let (m, d) = rest.split_once('-')?;
        (parse_u32(y)?, parse_u32(m)?, parse_u32(d)?)
    } else if date_part.len() == 8 && date_part.chars().all(|c| c.is_ascii_digit()) {
        (
            parse_u32(&date_part[..4])?,
            parse_u32(&date_part[4..6])?,
            parse_u32(&date_part[6..8])?,
        )
    } else {
        return None;
    };

    if year == 0 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// Parse a decimal `u32`, rejecting empty or non-numeric input.
fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Parse a Things 3 repeat rule into a [`RecurrenceSpec`].
///
/// Supports `daily`, `weekly`, `monthly`, `yearly`, `<N>d` (every N days),
/// and `<N>w` (every N weeks).
fn parse_repeat(raw: &str) -> Option<RecurrenceSpec> {
    let trimmed = raw.trim().to_lowercase();
    let pattern = match trimmed.as_str() {
        "daily" | "day" | "every day" => RecurrencePattern::Daily,
        "weekly" | "week" | "every week" => RecurrencePattern::Weekly,
        "monthly" | "month" | "every month" => RecurrencePattern::Monthly,
        "yearly" | "year" | "annual" | "every year" => RecurrencePattern::Yearly,
        other => {
            if let Some(n) = other.strip_suffix('d') {
                let n: u32 = n.trim().parse().ok()?;
                if n == 0 {
                    return None;
                }
                RecurrencePattern::EveryNDays(n)
            } else if let Some(n) = other.strip_suffix('w') {
                let n: u32 = n.trim().parse().ok()?;
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

/// Today's date as `YYYY-MM-DD` (UTC).
fn today_string() -> String {
    let today = OffsetDateTime::now_utc().date();
    format!(
        "{:04}-{:02}-{:02}",
        today.year(),
        u8::from(today.month()),
        today.day()
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;

    use super::{import_things3, map_status, parse_repeat, parse_things_date, today_string};
    use tock_core::domain::recurrence::RecurrencePattern;
    use tock_core::domain::task::TaskStatus;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        tock_storage::migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    // ── Date parsing ──────────────────────────────────────────────

    #[test]
    fn parses_iso_date() {
        assert_eq!(
            parse_things_date("2026-01-15"),
            Some(String::from("2026-01-15"))
        );
    }

    #[test]
    fn parses_iso_datetime() {
        assert_eq!(
            parse_things_date("2026-01-15T09:30:00"),
            Some(String::from("2026-01-15"))
        );
    }

    #[test]
    fn parses_compact_date() {
        assert_eq!(
            parse_things_date("20260115"),
            Some(String::from("2026-01-15"))
        );
    }

    #[test]
    fn rejects_invalid_month() {
        assert_eq!(parse_things_date("2026-13-01"), None);
    }

    #[test]
    fn rejects_garbage_date() {
        assert_eq!(parse_things_date("not-a-date"), None);
    }

    // ── Status mapping ────────────────────────────────────────────

    #[test]
    fn maps_statuses() {
        assert_eq!(map_status(Some("open")), TaskStatus::Pending);
        assert_eq!(map_status(None), TaskStatus::Pending);
        assert_eq!(map_status(Some("completed")), TaskStatus::Done);
        assert_eq!(map_status(Some("Canceled")), TaskStatus::Cancelled);
        assert_eq!(map_status(Some("cancelled")), TaskStatus::Cancelled);
    }

    // ── Repeat parsing ────────────────────────────────────────────

    #[test]
    fn parses_named_repeat() {
        assert_eq!(
            parse_repeat("weekly").map(|s| s.pattern),
            Some(RecurrencePattern::Weekly)
        );
    }

    #[test]
    fn parses_interval_repeat() {
        assert_eq!(
            parse_repeat("3d").map(|s| s.pattern),
            Some(RecurrencePattern::EveryNDays(3))
        );
        assert_eq!(
            parse_repeat("2w").map(|s| s.pattern),
            Some(RecurrencePattern::EveryNWeeks(2))
        );
    }

    #[test]
    fn rejects_unknown_repeat() {
        assert!(parse_repeat("fortnightly").is_none());
        assert!(parse_repeat("0d").is_none());
    }

    #[test]
    fn today_string_is_iso() {
        let today = today_string();
        assert_eq!(today.len(), 10);
        assert_eq!(&today[4..5], "-");
        assert_eq!(&today[7..8], "-");
    }

    // ── End-to-end import ─────────────────────────────────────────

    const SAMPLE: &str = r#"
    {
      "areas": [
        {
          "title": "Work",
          "projects": [
            {
              "title": "Website Redesign",
              "notes": "Q3 initiative",
              "deadline": "2026-08-01",
              "headings": [
                { "uuid": "head-design", "title": "Design" }
              ],
              "todos": [
                {
                  "title": "Draft wireframes",
                  "notes": "cover homepage",
                  "when": "2026-07-05",
                  "deadline": "2026-07-10",
                  "tags": ["design/ui"],
                  "heading": "head-design",
                  "checklist": [
                    { "title": "Homepage", "completed": true },
                    { "title": "About page" }
                  ]
                },
                {
                  "title": "Ship it",
                  "status": "completed",
                  "when": "today"
                }
              ]
            }
          ],
          "todos": [
            { "title": "Weekly review", "when": "evening", "repeat": "weekly" }
          ]
        }
      ],
      "projects": [
        { "title": "Personal Errands", "todos": [ { "title": "Buy milk" } ] }
      ],
      "todos": [
        { "title": "Read a book", "when": "someday" },
        { "title": "In the trash", "trashed": true }
      ]
    }
    "#;

    #[test]
    fn imports_full_hierarchy() {
        let mut conn = test_conn();
        let report = import_things3(&mut conn, SAMPLE, false).expect("import");

        // Areas, projects, headings.
        assert_eq!(report.areas_created, vec![String::from("Work")]);
        assert!(
            report
                .projects_created
                .contains(&String::from("Website Redesign"))
        );
        assert!(
            report
                .projects_created
                .contains(&String::from("Personal Errands"))
        );
        assert_eq!(report.headings_created, 1);

        // Tasks: wireframes, ship it, weekly review, buy milk, read a book.
        // The trashed one is skipped.
        assert_eq!(report.tasks_imported, 5);
        assert_eq!(report.trashed_skipped, 1);
        assert_eq!(report.checklist_items_degraded, 2);

        // Verify persisted structure.
        let areas = tock_storage::repo::area_repo::list(&conn, true).expect("areas");
        assert_eq!(areas.len(), 1);
        assert_eq!(areas[0].name, "Work");

        let projects = tock_storage::repo::project_repo::list(&conn, true).expect("projects");
        assert_eq!(projects.len(), 2);
        let web = projects
            .iter()
            .find(|p| p.name == "Website Redesign")
            .expect("web project");
        assert_eq!(web.area_id, Some(areas[0].id));
        assert_eq!(web.deadline.as_deref(), Some("2026-08-01"));

        let headings =
            tock_storage::repo::heading_repo::list_for_project(&conn, web.id).expect("headings");
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].name, "Design");

        // The wireframes task: dates, tags, heading, notes+checklist.
        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("tasks");
        let wire = tasks
            .iter()
            .find(|t| t.title == "Draft wireframes")
            .expect("wireframes task");
        assert_eq!(wire.start_date.as_deref(), Some("2026-07-05"));
        assert_eq!(wire.deadline.as_deref(), Some("2026-07-10"));
        assert_eq!(wire.heading_id, Some(headings[0].id));
        assert!(wire.tags.iter().any(|t| t == "design/ui"));
        let notes = wire.notes.as_deref().unwrap_or_default();
        assert!(notes.contains("cover homepage"));
        assert!(notes.contains("- [x] Homepage"));
        assert!(notes.contains("- [ ] About page"));

        // Completed task keeps Done status.
        let ship = tasks.iter().find(|t| t.title == "Ship it").expect("ship");
        assert_eq!(ship.status, TaskStatus::Done);

        // Evening task gets today + evening + weekly recurrence.
        let review = tasks
            .iter()
            .find(|t| t.title == "Weekly review")
            .expect("review");
        assert!(review.evening);
        assert_eq!(review.start_date.as_deref(), Some(today_string().as_str()));
        assert!(review.recurrence.is_some());

        // Someday task gets Someday status + tag.
        let book = tasks
            .iter()
            .find(|t| t.title == "Read a book")
            .expect("book");
        assert_eq!(book.status, TaskStatus::Someday);
        assert!(book.tags.iter().any(|t| t == "someday"));
    }

    #[test]
    fn include_trash_imports_trashed_items() {
        let mut conn = test_conn();
        let report = import_things3(&mut conn, SAMPLE, true).expect("import");
        assert_eq!(report.tasks_imported, 6);
        assert_eq!(report.trashed_skipped, 0);
    }

    #[test]
    fn rejects_malformed_json() {
        let mut conn = test_conn();
        assert!(import_things3(&mut conn, "{ not json", false).is_err());
    }
}
