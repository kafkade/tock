//! Output formatters for task display.

use std::fmt::Write as _;

use serde::Serialize;
use tock_core::domain::task::Task;

/// Output format selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFormat {
    /// Render tasks in a fixed-width table.
    Table,
    /// Render tasks as one compact line each.
    Compact,
    /// Render tasks as JSON.
    Json,
}

impl OutputFormat {
    /// Parse an output format string, defaulting to table for unknown values.
    #[allow(clippy::should_implement_trait)]
    #[must_use]
    pub const fn from_str(s: &str) -> Self {
        if s.eq_ignore_ascii_case("compact") {
            Self::Compact
        } else if s.eq_ignore_ascii_case("json") {
            Self::Json
        } else {
            Self::Table
        }
    }
}

#[derive(Serialize)]
struct TaskJson<'a> {
    sid: u32,
    title: &'a str,
    status: &'a str,
    priority: Option<char>,
    deadline: Option<&'a str>,
    tags: &'a [String],
    udas: &'a std::collections::BTreeMap<String, serde_json::Value>,
    created_at: String,
}

/// Format a list of tasks in the given format.
#[must_use]
pub fn format_tasks(tasks: &[Task], fmt: OutputFormat) -> String {
    match fmt {
        OutputFormat::Table => {
            let mut lines = Vec::with_capacity(tasks.len().saturating_add(1));
            lines.push(format_task_header());
            lines.extend(tasks.iter().map(format_task_row));
            lines.join("\n")
        }
        OutputFormat::Compact => tasks
            .iter()
            .map(format_task_compact)
            .collect::<Vec<_>>()
            .join("\n"),
        OutputFormat::Json => {
            let json_tasks = tasks.iter().map(task_json).collect::<Vec<_>>();
            serde_json::to_string(&json_tasks)
                .map_or_else(|_| String::from("[]"), std::convert::identity)
        }
    }
}

/// Format a single task detail.
#[must_use]
pub fn format_task_detail(task: &Task, fmt: OutputFormat) -> String {
    match fmt {
        OutputFormat::Table => format_task_detail_table(task),
        OutputFormat::Compact => format_task_compact(task),
        OutputFormat::Json => serde_json::to_string(&task_json(task))
            .map_or_else(|_| String::from("{}"), std::convert::identity),
    }
}

fn task_json(task: &Task) -> TaskJson<'_> {
    TaskJson {
        sid: task.sid,
        title: &task.title,
        status: task.status.as_str(),
        priority: task
            .priority
            .as_ref()
            .map(tock_core::domain::task::Priority::as_char),
        deadline: task.deadline.as_deref(),
        tags: &task.tags,
        udas: &task.udas.0,
        created_at: task.created_at.to_string(),
    }
}

fn format_task_detail_table(task: &Task) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Task #{} — {}", task.sid, task.title));
    lines.push(format!("  Status:   {}", task.status.as_str()));
    if let Some(ref priority) = task.priority {
        lines.push(format!("  Priority: {}", priority.as_char()));
    }
    if let Some(ref deadline) = task.deadline {
        lines.push(format!("  Deadline: {deadline}"));
    }
    if let Some(ref start_date) = task.start_date {
        lines.push(format!("  Start:    {start_date}"));
    }
    if !task.tags.is_empty() {
        let tag_str = task
            .tags
            .iter()
            .map(|tag| format!("#{tag}"))
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(format!("  Tags:     {tag_str}"));
    }
    if !task.udas.0.is_empty() {
        lines.push(String::from("  UDAs:"));
        for (key, value) in &task.udas.0 {
            lines.push(format!("    {key}: {value}"));
        }
    }
    if let Some(ref notes) = task.notes {
        lines.push(format!("  Notes:    {notes}"));
    }
    lines.push(format!("  Created:  {}", task.created_at));
    lines.push(format!("  Modified: {}", task.modified_at));
    if let Some(ref done_at) = task.done_at {
        lines.push(format!("  Done at:  {done_at}"));
    }
    lines.join("\n")
}

fn format_task_row(task: &Task) -> String {
    let priority = task
        .priority
        .as_ref()
        .map_or_else(|| String::from(" "), |value| value.as_char().to_string());
    let tags = if task.tags.is_empty() {
        String::new()
    } else {
        task.tags
            .iter()
            .map(|tag| format!("#{tag}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let deadline = task.deadline.as_deref().unwrap_or("");
    format!(
        "{:>4}  {:<1}  {:<7}  {:<40}  {:<12}  {}",
        task.sid,
        priority,
        task.status.as_str(),
        truncate(&task.title, 40),
        deadline,
        tags,
    )
}

fn format_task_header() -> String {
    format!(
        "{:>4}  {:<1}  {:<7}  {:<40}  {:<12}  {}",
        "SID", "P", "Status", "Title", "Deadline", "Tags"
    )
}

fn format_task_compact(task: &Task) -> String {
    let mut line = format!("#{} [{}] {}", task.sid, task.status.as_str(), task.title);
    if let Some(priority) = task.priority {
        let _ = write!(line, " ({})", priority.as_char());
    }
    if let Some(deadline) = task.deadline.as_deref() {
        let _ = write!(line, " due:{deadline}");
    }
    if !task.tags.is_empty() {
        let tags = task
            .tags
            .iter()
            .map(|tag| format!("#{tag}"))
            .collect::<Vec<_>>()
            .join(" ");
        line.push(' ');
        line.push_str(&tags);
    }
    line
}

fn truncate(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max {
        s.to_string()
    } else {
        let mut truncated = s.chars().take(max.saturating_sub(1)).collect::<String>();
        truncated.push('…');
        truncated
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{OutputFormat, format_task_detail, format_tasks};
    use time::OffsetDateTime;
    use tock_core::domain::task::{Priority, Task, TaskStatus};
    use tock_core::domain::uda::UdaValues;
    use uuid::Uuid;

    fn sample_task() -> Task {
        Task {
            id: Uuid::nil(),
            sid: 1,
            title: String::from("Buy groceries"),
            notes: Some(String::from("Milk")),
            status: TaskStatus::Inbox,
            area_id: None,
            project_id: None,
            heading_id: None,
            parent_id: None,
            start_date: None,
            deadline: Some(String::from("2026-06-01")),
            recurrence: None,
            priority: Some(Priority::High),
            evening: false,
            udas: {
                let mut values = UdaValues::default();
                values.set("effort", serde_json::json!(3));
                values
            },
            tags: vec![String::from("errands")],
            depends_on: Vec::new(),
            urgency: 0.0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            modified_at: OffsetDateTime::UNIX_EPOCH,
            done_at: None,
            cancelled_at: None,
            deleted_at: None,
        }
    }

    #[test]
    fn parses_output_formats() {
        assert_eq!(OutputFormat::from_str("table"), OutputFormat::Table);
        assert_eq!(OutputFormat::from_str("compact"), OutputFormat::Compact);
        assert_eq!(OutputFormat::from_str("json"), OutputFormat::Json);
        assert_eq!(OutputFormat::from_str("unknown"), OutputFormat::Table);
    }

    #[test]
    fn formats_compact_tasks() {
        let rendered = format_tasks(&[sample_task()], OutputFormat::Compact);
        assert_eq!(
            rendered,
            "#1 [inbox] Buy groceries (H) due:2026-06-01 #errands"
        );
    }

    #[test]
    fn formats_json_task_detail() {
        let rendered = format_task_detail(&sample_task(), OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(parsed["sid"], 1);
        assert_eq!(parsed["title"], "Buy groceries");
        assert_eq!(parsed["status"], "inbox");
        assert_eq!(parsed["priority"], "H");
        assert_eq!(parsed["deadline"], "2026-06-01");
        assert_eq!(parsed["tags"][0], "errands");
        assert_eq!(parsed["udas"]["effort"], 3);
    }

    #[test]
    fn formats_human_task_detail_with_udas() {
        let rendered = format_task_detail(&sample_task(), OutputFormat::Table);
        assert!(rendered.contains("UDAs:"));
        assert!(rendered.contains("effort: 3"));
    }
}
