//! Project domain model per architecture §2.1.1.

use time::OffsetDateTime;
use uuid::Uuid;

/// Project status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectStatus {
    /// Active project.
    Active,
    /// Paused (on hold).
    Paused,
    /// Deferred indefinitely.
    Someday,
    /// Completed.
    Done,
    /// Abandoned.
    Cancelled,
}

impl ProjectStatus {
    /// Canonical string for storage.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Someday => "someday",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parse from storage string.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        Some(match s {
            "active" => Self::Active,
            "paused" => Self::Paused,
            "someday" => Self::Someday,
            "done" => Self::Done,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }
}

/// A project — has a goal and a completion state.
#[derive(Clone, Debug)]
pub struct Project {
    /// Globally unique identifier.
    pub id: Uuid,
    /// Short workspace-local identifier.
    pub sid: u32,
    /// Project name.
    pub name: String,
    /// Markdown notes.
    pub notes: Option<String>,
    /// Current status.
    pub status: ProjectStatus,
    /// Area this project belongs to.
    pub area_id: Option<Uuid>,
    /// Hard deadline.
    pub deadline: Option<String>,
    /// Sort position.
    pub sort_order: i32,
    /// When created.
    pub created_at: OffsetDateTime,
    /// When last modified.
    pub modified_at: OffsetDateTime,
    /// When archived (if archived).
    pub archived_at: Option<OffsetDateTime>,
}

/// Input for creating a new project.
#[derive(Clone, Debug)]
pub struct NewProject {
    /// Name (required).
    pub name: String,
    /// Notes.
    pub notes: Option<String>,
    /// Area to assign to.
    pub area_id: Option<Uuid>,
    /// Deadline.
    pub deadline: Option<String>,
}
