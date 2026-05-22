//! Habit domain model per architecture §2.2.

use time::OffsetDateTime;
use uuid::Uuid;

/// Direction of the habit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HabitDirection {
    /// Building a new positive habit.
    Build,
    /// Breaking a negative habit.
    Break,
}

impl HabitDirection {
    /// Canonical storage representation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Break => "break",
        }
    }

    /// Parse from the canonical storage representation.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "build" => Some(Self::Build),
            "break" => Some(Self::Break),
            _ => None,
        }
    }
}

/// Cadence — how often the habit should be performed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cadence {
    /// Due every day.
    Daily,
    /// Due a target number of times per calendar week.
    WeeklyTarget {
        /// Number of successful periods required each week.
        times_per_week: u8,
    },
    /// Due on specific named weekdays.
    SpecificDays {
        /// Weekday names, stored as strings for a simple CLI-first path.
        days: Vec<String>,
    },
    /// Due every `n` calendar days.
    EveryNDays {
        /// Days between successive due dates.
        n: u8,
    },
}

/// Minimum threshold — the "start small" amount.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Minimum {
    /// Count-based minimum, such as pages or reps.
    Count(u32),
    /// Duration-based minimum in minutes.
    DurationMinutes(u32),
    /// Boolean completion with no numeric quantity.
    Boolean,
}

/// A habit.
#[derive(Clone, Debug)]
pub struct Habit {
    /// Globally unique identifier (`UUIDv7`).
    pub id: Uuid,
    /// Short workspace-local identifier for CLI ergonomics.
    pub sid: u32,
    /// Habit title.
    pub title: String,
    /// Optional identity statement, such as "I am someone who exercises daily".
    pub identity: Option<String>,
    /// Cue or implementation intention.
    pub cue: Option<String>,
    /// Craving or motivation framing.
    pub craving: Option<String>,
    /// Response or action definition.
    pub response: Option<String>,
    /// Reward after completion.
    pub reward: Option<String>,
    /// Whether the habit is a build or break habit.
    pub direction: HabitDirection,
    /// JSON-serialized cadence payload.
    pub cadence: String,
    /// JSON-serialized minimum payload.
    pub minimum: String,
    /// Parent habit for habit stacking, when present.
    pub stack_after: Option<Uuid>,
    /// Delay in seconds after the parent habit completes.
    pub stack_delay_s: u32,
    /// Related area, if any.
    pub area_id: Option<Uuid>,
    /// Related project, if any.
    pub project_id: Option<Uuid>,
    /// Current progression level.
    pub level: u32,
    /// Total accumulated XP.
    pub xp: u32,
    /// Current streak in cadence periods.
    pub streak_current: u32,
    /// Best streak recorded so far.
    pub streak_best: u32,
    /// When the habit was created.
    pub created_at: OffsetDateTime,
    /// When the habit was last modified.
    pub modified_at: OffsetDateTime,
    /// When the habit was archived, if archived.
    pub archived_at: Option<OffsetDateTime>,
}

impl Habit {
    /// Return the display name of the habit's current level.
    #[must_use]
    pub const fn level_name(&self) -> &'static str {
        match self.level {
            1 => "Spark",
            2 => "Starter",
            3 => "Established",
            4 => "Steady",
            5 => "Anchored",
            6 => "Identity",
            7.. => "Embodied",
            _ => "Unknown",
        }
    }

    /// Whether this is a break-bad-habit.
    #[must_use]
    pub const fn is_break(&self) -> bool {
        matches!(self.direction, HabitDirection::Break)
    }

    /// Streak label: "days clean" for break habits, "streak" for build.
    #[must_use]
    pub const fn streak_label(&self) -> &'static str {
        if self.is_break() { "clean" } else { "streak" }
    }

    /// Emoji prefix for display.
    #[must_use]
    pub const fn direction_emoji(&self) -> &'static str {
        if self.is_break() { "🚫" } else { "✅" }
    }
}

/// A reminder configuration for a habit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reminder {
    /// Time of day (HH:MM format).
    pub time: String,
    /// Days this reminder applies to (empty = every day).
    pub days: Vec<String>,
}

impl Reminder {
    /// Parse a JSON array of reminders.
    #[must_use]
    pub fn from_json_array(s: &str) -> Vec<Self> {
        // Expected format: [{"time":"07:00","days":["monday","wednesday"]}]
        let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(s) else {
            return Vec::new();
        };
        arr.iter()
            .filter_map(|v| {
                let time = v.get("time")?.as_str()?.to_string();
                let days = v
                    .get("days")
                    .and_then(|d| d.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|d| d.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                Some(Self { time, days })
            })
            .collect()
    }

    /// Serialize a list of reminders to JSON.
    #[must_use]
    pub fn to_json_array(reminders: &[Self]) -> String {
        let arr: Vec<serde_json::Value> = reminders
            .iter()
            .map(|r| {
                serde_json::json!({
                    "time": r.time,
                    "days": r.days,
                })
            })
            .collect();
        serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string())
    }

    /// Human-readable display.
    #[must_use]
    pub fn display(&self) -> String {
        if self.days.is_empty() {
            format!("daily at {}", self.time)
        } else {
            format!("{} at {}", self.days.join("/"), self.time)
        }
    }
}

/// Input for creating a new habit.
#[derive(Clone, Debug)]
pub struct NewHabit {
    /// Habit title.
    pub title: String,
    /// Optional identity statement.
    pub identity: Option<String>,
    /// Cue or implementation intention.
    pub cue: Option<String>,
    /// Craving or motivation framing.
    pub craving: Option<String>,
    /// Response or action definition.
    pub response: Option<String>,
    /// Reward after completion.
    pub reward: Option<String>,
    /// Whether the habit is a build or break habit.
    pub direction: HabitDirection,
    /// JSON-serialized cadence payload.
    pub cadence: String,
    /// JSON-serialized minimum payload.
    pub minimum: String,
    /// Parent habit SID for habit stacking.
    pub stack_after: Option<u32>,
    /// Delay in seconds after the parent habit completes.
    pub stack_delay_s: u32,
    /// Related area, if any.
    pub area_id: Option<Uuid>,
    /// Related project, if any.
    pub project_id: Option<Uuid>,
}

/// Fields that can be patched on a habit.
#[derive(Clone, Debug, Default)]
pub struct HabitPatch {
    /// New title.
    pub title: Option<String>,
    /// New identity statement.
    pub identity: Option<Option<String>>,
    /// New cue.
    pub cue: Option<Option<String>>,
    /// New craving.
    pub craving: Option<Option<String>>,
    /// New response.
    pub response: Option<Option<String>>,
    /// New reward.
    pub reward: Option<Option<String>>,
    /// New parent habit SID for stacking.
    pub stack_after: Option<Option<u32>>,
    /// New stack delay in seconds.
    pub stack_delay_s: Option<u32>,
}

/// A habit log entry.
#[derive(Clone, Debug)]
pub struct HabitEntry {
    /// Globally unique identifier (`UUIDv7`).
    pub id: Uuid,
    /// Linked habit identifier.
    pub habit_id: Uuid,
    /// When the entry occurred.
    pub occurred_at: OffsetDateTime,
    /// JSON-serialized entry amount payload.
    pub amount: String,
    /// Optional freeform notes.
    pub notes: Option<String>,
    /// Whether this entry records a slip for a break habit.
    pub slip: bool,
    /// Source of the log entry, such as `cli` or `timer`.
    pub source: String,
    /// When the log row was created.
    pub created_at: OffsetDateTime,
}

/// A skip or freeze day recorded for a habit.
#[derive(Clone, Debug)]
pub struct HabitSkip {
    /// Globally unique identifier (`UUIDv7`).
    pub id: Uuid,
    /// Linked habit identifier.
    pub habit_id: Uuid,
    /// ISO date string for the skipped day.
    pub date: String,
    /// Skip type, such as `skip` or `freeze`.
    pub kind: String,
    /// Optional explanation for the skip.
    pub reason: Option<String>,
    /// When the skip row was created.
    pub created_at: OffsetDateTime,
}

#[cfg(test)]
mod tests {
    use super::{Habit, HabitDirection};
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[test]
    fn direction_roundtrip() {
        for direction in [HabitDirection::Build, HabitDirection::Break] {
            assert_eq!(
                HabitDirection::from_str_opt(direction.as_str()),
                Some(direction)
            );
        }
    }

    #[test]
    fn level_names_match_thresholds() {
        let mut habit = Habit {
            id: Uuid::nil(),
            sid: 1,
            title: String::from("Read"),
            identity: None,
            cue: None,
            craving: None,
            response: None,
            reward: None,
            direction: HabitDirection::Build,
            cadence: String::from("\"daily\""),
            minimum: String::from("\"boolean\""),
            stack_after: None,
            stack_delay_s: 0,
            area_id: None,
            project_id: None,
            level: 1,
            xp: 0,
            streak_current: 0,
            streak_best: 0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            modified_at: OffsetDateTime::UNIX_EPOCH,
            archived_at: None,
        };

        let expected = [
            (1, "Spark"),
            (2, "Starter"),
            (3, "Established"),
            (4, "Steady"),
            (5, "Anchored"),
            (6, "Identity"),
            (7, "Embodied"),
            (8, "Embodied"),
        ];

        for (level, name) in expected {
            habit.level = level;
            assert_eq!(habit.level_name(), name);
        }
    }
}
