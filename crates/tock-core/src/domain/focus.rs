//! Focus (Pomodoro) session domain model per architecture §2.4.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Focus session state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusState {
    /// The session is in a work interval.
    Working,
    /// The session is in a short break interval.
    ShortBreak,
    /// The session is in a long break interval.
    LongBreak,
    /// The session is paused.
    Paused,
    /// The session ended early.
    Aborted,
    /// The session completed all planned cycles.
    Completed,
}

impl FocusState {
    /// Canonical storage representation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::ShortBreak => "short_break",
            Self::LongBreak => "long_break",
            Self::Paused => "paused",
            Self::Aborted => "aborted",
            Self::Completed => "completed",
        }
    }

    /// Parse from the canonical storage representation.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        Some(match s {
            "working" => Self::Working,
            "short_break" => Self::ShortBreak,
            "long_break" => Self::LongBreak,
            "paused" => Self::Paused,
            "aborted" => Self::Aborted,
            "completed" => Self::Completed,
            _ => return None,
        })
    }

    /// Whether this state is terminal.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Aborted | Self::Completed)
    }
}

/// Focus session configuration snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusConfig {
    /// Length of each work interval in minutes.
    pub work_minutes: u32,
    /// Length of each short break in minutes.
    pub short_break_minutes: u32,
    /// Length of each long break in minutes.
    pub long_break_minutes: u32,
    /// Number of completed work cycles before a long break.
    pub cycles_before_long_break: u32,
}

impl Default for FocusConfig {
    fn default() -> Self {
        Self {
            work_minutes: 25,
            short_break_minutes: 5,
            long_break_minutes: 15,
            cycles_before_long_break: 4,
        }
    }
}

/// A focus (Pomodoro) session.
#[derive(Clone, Debug)]
pub struct FocusSession {
    /// Globally unique identifier (`UUIDv7`).
    pub id: Uuid,
    /// Short workspace-local identifier for CLI ergonomics.
    pub sid: u32,
    /// When the session started.
    pub started_at: OffsetDateTime,
    /// When the session ended, if terminal.
    pub ended_at: Option<OffsetDateTime>,
    /// Linked task identifier, if any.
    pub task_id: Option<Uuid>,
    /// Linked project identifier, if any.
    pub project_id: Option<Uuid>,
    /// Total number of planned work cycles.
    pub planned_cycles: u32,
    /// Number of completed work cycles.
    pub completed_cycles: u32,
    /// Current lifecycle state.
    pub state: FocusState,
    /// Configuration snapshot captured at session start.
    pub config: FocusConfig,
    /// When the session row was created.
    pub created_at: OffsetDateTime,
    /// When the session row was last modified.
    pub modified_at: OffsetDateTime,
}

/// Input for creating a new focus session.
#[derive(Clone, Debug)]
pub struct NewFocusSession {
    /// Linked task identifier, if any.
    pub task_id: Option<Uuid>,
    /// Linked project identifier, if any.
    pub project_id: Option<Uuid>,
    /// Total number of planned work cycles.
    pub planned_cycles: u32,
    /// Configuration snapshot captured at session start.
    pub config: FocusConfig,
}

#[cfg(test)]
mod tests {
    use super::{FocusConfig, FocusState};

    #[test]
    fn state_roundtrip() {
        let states = [
            FocusState::Working,
            FocusState::ShortBreak,
            FocusState::LongBreak,
            FocusState::Paused,
            FocusState::Aborted,
            FocusState::Completed,
        ];

        for state in states {
            assert_eq!(FocusState::from_str_opt(state.as_str()), Some(state));
        }
    }

    #[test]
    fn default_config_matches_pomodoro_defaults() {
        assert_eq!(
            FocusConfig::default(),
            FocusConfig {
                work_minutes: 25,
                short_break_minutes: 5,
                long_break_minutes: 15,
                cycles_before_long_break: 4,
            }
        );
    }
}
