//! Task domain model per architecture §2.1.

use std::collections::BTreeMap;

use time::OffsetDateTime;
use uuid::Uuid;

use crate::domain::uda::UdaValues;

/// Task status — the lifecycle states a task can be in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    /// New, unprocessed.
    Inbox,
    /// Ready to be worked on.
    Pending,
    /// Actively being worked on.
    Started,
    /// Completed.
    Done,
    /// Abandoned.
    Cancelled,
    /// Deferred indefinitely.
    Someday,
}

impl TaskStatus {
    /// Canonical string representation for storage.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Pending => "pending",
            Self::Started => "started",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
            Self::Someday => "someday",
        }
    }

    /// Parse from the canonical storage string.
    ///
    /// # Errors
    /// Returns `None` if the string doesn't match any known status.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        Some(match s {
            "inbox" => Self::Inbox,
            "pending" => Self::Pending,
            "started" => Self::Started,
            "done" => Self::Done,
            "cancelled" => Self::Cancelled,
            "someday" => Self::Someday,
            _ => return None,
        })
    }

    /// Whether this status represents a "closed" state.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        matches!(self, Self::Done | Self::Cancelled)
    }
}

/// Priority level (optional).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    /// Low.
    Low,
    /// Medium.
    Medium,
    /// High.
    High,
}

impl Priority {
    /// Single-character canonical form.
    #[must_use]
    pub const fn as_char(&self) -> char {
        match self {
            Self::Low => 'L',
            Self::Medium => 'M',
            Self::High => 'H',
        }
    }

    /// Parse from `L`, `M`, `H` (case-insensitive).
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        Some(match s.to_uppercase().as_str() {
            "L" => Self::Low,
            "M" => Self::Medium,
            "H" => Self::High,
            _ => return None,
        })
    }
}

/// A task — the atomic unit of work in tock.
#[derive(Clone, Debug)]
pub struct Task {
    /// Globally unique identifier (`UUIDv7`).
    pub id: Uuid,
    /// Short workspace-local identifier for CLI ergonomics.
    pub sid: u32,
    /// Task title.
    pub title: String,
    /// Markdown notes.
    pub notes: Option<String>,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// Area this task belongs to (optional).
    pub area_id: Option<Uuid>,
    /// Project this task belongs to (optional).
    pub project_id: Option<Uuid>,
    /// Heading within a project (optional).
    pub heading_id: Option<Uuid>,
    /// Template task this instance was generated from.
    pub parent_id: Option<Uuid>,
    /// Deferred start date.
    pub start_date: Option<String>,
    /// Hard deadline.
    pub deadline: Option<String>,
    /// Stored recurrence specification as JSON.
    pub recurrence: Option<String>,
    /// Priority level.
    pub priority: Option<Priority>,
    /// Whether this is an "evening" task.
    pub evening: bool,
    /// User-defined attributes (JSON key-value pairs).
    pub udas: UdaValues,
    /// Tags (flat list of tag names).
    pub tags: Vec<String>,
    /// Tasks this task depends on.
    pub depends_on: Vec<Uuid>,
    /// Cached urgency score (recomputed on write).
    pub urgency: f64,
    /// When the task was created.
    pub created_at: OffsetDateTime,
    /// When the task was last modified.
    pub modified_at: OffsetDateTime,
    /// When the task was completed (if done).
    pub done_at: Option<OffsetDateTime>,
    /// When the task was cancelled (if cancelled).
    pub cancelled_at: Option<OffsetDateTime>,
    /// When the task was soft-deleted.
    pub deleted_at: Option<OffsetDateTime>,
}

/// Input for creating a new task.
#[derive(Clone, Debug, Default)]
pub struct NewTask {
    /// Title (required).
    pub title: String,
    /// Notes (optional).
    pub notes: Option<String>,
    /// Initial status (defaults to Inbox).
    pub status: Option<TaskStatus>,
    /// Project to assign to.
    pub project_id: Option<Uuid>,
    /// Area to assign to.
    pub area_id: Option<Uuid>,
    /// Heading within the project.
    pub heading_id: Option<Uuid>,
    /// Template task this instance was generated from.
    pub parent_id: Option<Uuid>,
    /// Deferred start date.
    pub start_date: Option<String>,
    /// Deadline.
    pub deadline: Option<String>,
    /// Stored recurrence specification as JSON.
    pub recurrence: Option<String>,
    /// Priority.
    pub priority: Option<Priority>,
    /// Evening flag.
    pub evening: bool,
    /// Initial UDA values.
    pub udas: UdaValues,
    /// Tags to apply.
    pub tags: Vec<String>,
}

/// Fields that can be modified on an existing task.
#[derive(Clone, Debug, Default)]
pub struct TaskPatch {
    /// New title.
    pub title: Option<String>,
    /// New notes.
    pub notes: Option<Option<String>>,
    /// New status.
    pub status: Option<TaskStatus>,
    /// New project.
    pub project_id: Option<Option<Uuid>>,
    /// New area.
    pub area_id: Option<Option<Uuid>>,
    /// New heading.
    pub heading_id: Option<Option<Uuid>>,
    /// New start date.
    pub start_date: Option<Option<String>>,
    /// New deadline.
    pub deadline: Option<Option<String>>,
    /// New priority.
    pub priority: Option<Option<Priority>>,
    /// New evening flag.
    pub evening: Option<bool>,
    /// UDA values to set (merged with existing values).
    pub set_udas: BTreeMap<String, serde_json::Value>,
    /// UDA keys to remove.
    pub remove_udas: Vec<String>,
    /// Tags to add.
    pub add_tags: Vec<String>,
    /// Tags to remove.
    pub remove_tags: Vec<String>,
    /// Dependency SIDs to add.
    pub add_deps: Vec<u32>,
    /// Dependency SIDs to remove.
    pub remove_deps: Vec<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_roundtrip() {
        let statuses = [
            TaskStatus::Inbox,
            TaskStatus::Pending,
            TaskStatus::Started,
            TaskStatus::Done,
            TaskStatus::Cancelled,
            TaskStatus::Someday,
        ];
        for s in statuses {
            assert_eq!(TaskStatus::from_str_opt(s.as_str()), Some(s));
        }
    }

    #[test]
    fn priority_roundtrip() {
        for p in [Priority::Low, Priority::Medium, Priority::High] {
            let c = String::from(p.as_char());
            assert_eq!(Priority::from_str_opt(&c), Some(p));
        }
    }

    #[test]
    fn closed_status() {
        assert!(TaskStatus::Done.is_closed());
        assert!(TaskStatus::Cancelled.is_closed());
        assert!(!TaskStatus::Pending.is_closed());
        assert!(!TaskStatus::Inbox.is_closed());
    }
}
