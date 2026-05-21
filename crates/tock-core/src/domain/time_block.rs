//! Time block domain model per architecture §2.3.

use time::OffsetDateTime;
use uuid::Uuid;

/// Source of how a time block was created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockSource {
    /// Created manually by the user.
    Manual,
    /// Created from the running timer command.
    Timer,
    /// Created from a Pomodoro session.
    Pomodoro,
    /// Imported from another system.
    Imported,
}

impl BlockSource {
    /// Canonical string representation for storage.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Timer => "timer",
            Self::Pomodoro => "pomodoro",
            Self::Imported => "imported",
        }
    }

    /// Parse from the canonical storage string.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        Some(match s {
            "manual" => Self::Manual,
            "timer" => Self::Timer,
            "pomodoro" => Self::Pomodoro,
            "imported" => Self::Imported,
            _ => return None,
        })
    }
}

/// A time block — a closed or open interval of attention.
#[derive(Clone, Debug)]
pub struct TimeBlock {
    /// Globally unique identifier (`UUIDv7`).
    pub id: Uuid,
    /// Short workspace-local identifier for CLI ergonomics.
    pub sid: u32,
    /// Block title.
    pub title: String,
    /// Block start timestamp.
    pub start_ts: OffsetDateTime,
    /// Block end timestamp. `None` means the block is still running.
    pub end_ts: Option<OffsetDateTime>,
    /// Linked project identifier, if any.
    pub project_id: Option<Uuid>,
    /// Linked task identifier, if any.
    pub task_id: Option<Uuid>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// How the block was created.
    pub source: BlockSource,
    /// Whether the block is billable.
    pub billable: bool,
    /// When the block row was created.
    pub created_at: OffsetDateTime,
    /// When the block row was last modified.
    pub modified_at: OffsetDateTime,
}

impl TimeBlock {
    /// Duration of this block. Returns `None` if still running.
    #[must_use]
    pub fn duration(&self) -> Option<time::Duration> {
        self.end_ts.map(|end_ts| end_ts - self.start_ts)
    }

    /// Whether this block is currently running (no end timestamp).
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.end_ts.is_none()
    }
}

/// Input for creating a new time block.
#[derive(Clone, Debug)]
pub struct NewTimeBlock {
    /// Block title.
    pub title: String,
    /// Linked task identifier, if any.
    pub task_id: Option<Uuid>,
    /// Linked project identifier, if any.
    pub project_id: Option<Uuid>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// How the block was created.
    pub source: BlockSource,
}

/// Fields that can be patched on an existing time block.
#[derive(Clone, Debug, Default)]
pub struct TimeBlockPatch {
    /// New title.
    pub title: Option<String>,
    /// New notes (`Some(None)` clears, `None` leaves unchanged).
    pub notes: Option<Option<String>>,
    /// New start timestamp (ISO 8601 string).
    pub start: Option<String>,
    /// New end timestamp (`Some(None)` reopens the block).
    pub end: Option<Option<String>>,
    /// New linked task.
    pub task_id: Option<Option<Uuid>>,
    /// New billable flag.
    pub billable: Option<bool>,
}
