//! `UniFFI`-compatible mirrors of `tock-core` domain types.
//!
//! These types use only primitives that `UniFFI` can bridge to Swift:
//! `String`, `u32`, `i32`, `f64`, `bool`, `Vec<T>`, `Option<T>`,
//! and other `UniFFI` records/enums.
//!
//! UUIDs are represented as hyphenated strings. Timestamps use RFC 3339
//! strings. JSON blobs (UDAs, cadence, minimum) stay as opaque strings.

// ── Enums ────────────────────────────────────────────────────────────

/// Task lifecycle status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TockTaskStatus {
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

/// Priority level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TockPriority {
    /// Low priority.
    Low,
    /// Medium priority.
    Medium,
    /// High priority.
    High,
}

/// Project lifecycle status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TockProjectStatus {
    /// Active project.
    Active,
    /// On hold.
    Paused,
    /// Deferred indefinitely.
    Someday,
    /// Completed.
    Done,
    /// Abandoned.
    Cancelled,
}

/// Focus session state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TockFocusState {
    /// In a work interval.
    Working,
    /// In a short break.
    ShortBreak,
    /// In a long break.
    LongBreak,
    /// Paused.
    Paused,
    /// Ended early.
    Aborted,
    /// All planned cycles finished.
    Completed,
}

/// How a time block was created.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TockBlockSource {
    /// Manually entered.
    Manual,
    /// Created from the running timer.
    Timer,
    /// Created from a Pomodoro focus session.
    Pomodoro,
    /// Imported from another system.
    Imported,
}

/// Habit direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TockHabitDirection {
    /// Building a new positive habit.
    Build,
    /// Breaking a negative habit.
    Break,
}

// ── Records ──────────────────────────────────────────────────────────

/// A task.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockTask {
    /// UUID string.
    pub id: String,
    /// Short workspace-local identifier.
    pub sid: u32,
    /// Task title.
    pub title: String,
    /// Markdown notes.
    pub notes: Option<String>,
    /// Current lifecycle status.
    pub status: TockTaskStatus,
    /// Area UUID string.
    pub area_id: Option<String>,
    /// Project UUID string.
    pub project_id: Option<String>,
    /// Heading UUID string.
    pub heading_id: Option<String>,
    /// Parent (template) task UUID string.
    pub parent_id: Option<String>,
    /// Deferred start date.
    pub start_date: Option<String>,
    /// Hard deadline.
    pub deadline: Option<String>,
    /// Recurrence specification (JSON).
    pub recurrence: Option<String>,
    /// Priority level.
    pub priority: Option<TockPriority>,
    /// Whether this is an "evening" task.
    pub evening: bool,
    /// User-defined attributes (JSON object string).
    pub udas: String,
    /// Tag names.
    pub tags: Vec<String>,
    /// UUIDs of tasks this task depends on.
    pub depends_on: Vec<String>,
    /// Cached urgency score.
    pub urgency: f64,
    /// Creation timestamp (RFC 3339).
    pub created_at: String,
    /// Last modification timestamp (RFC 3339).
    pub modified_at: String,
    /// Completion timestamp (RFC 3339).
    pub done_at: Option<String>,
    /// Cancellation timestamp (RFC 3339).
    pub cancelled_at: Option<String>,
    /// Soft-deletion timestamp (RFC 3339).
    pub deleted_at: Option<String>,
}

/// A project.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockProject {
    /// UUID string.
    pub id: String,
    /// Short workspace-local identifier.
    pub sid: u32,
    /// Project name.
    pub name: String,
    /// Markdown notes.
    pub notes: Option<String>,
    /// Current status.
    pub status: TockProjectStatus,
    /// Area UUID string.
    pub area_id: Option<String>,
    /// Hard deadline.
    pub deadline: Option<String>,
    /// Sort position.
    pub sort_order: i32,
    /// Creation timestamp (RFC 3339).
    pub created_at: String,
    /// Last modification timestamp (RFC 3339).
    pub modified_at: String,
    /// Archive timestamp (RFC 3339).
    pub archived_at: Option<String>,
}

/// An area (long-lived life domain).
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockArea {
    /// UUID string.
    pub id: String,
    /// Area name.
    pub name: String,
    /// Display color.
    pub color: Option<String>,
    /// Sort position.
    pub sort_order: i32,
    /// Creation timestamp (RFC 3339).
    pub created_at: String,
    /// Last modification timestamp (RFC 3339).
    pub modified_at: String,
    /// Archive timestamp (RFC 3339).
    pub archived_at: Option<String>,
}

/// A tag.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockTag {
    /// UUID string.
    pub id: String,
    /// Tag name.
    pub name: String,
    /// Display color.
    pub color: Option<String>,
}

/// A time block (closed or open interval of attention).
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockTimeBlock {
    /// UUID string.
    pub id: String,
    /// Short workspace-local identifier.
    pub sid: u32,
    /// Block title / description.
    pub title: String,
    /// Start timestamp (RFC 3339).
    pub start_ts: String,
    /// End timestamp (RFC 3339). `None` means still running.
    pub end_ts: Option<String>,
    /// Linked project UUID string.
    pub project_id: Option<String>,
    /// Linked task UUID string.
    pub task_id: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// How the block was created.
    pub source: TockBlockSource,
    /// Whether this block is billable.
    pub billable: bool,
    /// Creation timestamp (RFC 3339).
    pub created_at: String,
    /// Last modification timestamp (RFC 3339).
    pub modified_at: String,
}

/// Focus session configuration snapshot.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockFocusConfig {
    /// Work interval length in minutes.
    pub work_minutes: u32,
    /// Short break length in minutes.
    pub short_break_minutes: u32,
    /// Long break length in minutes.
    pub long_break_minutes: u32,
    /// Completed cycles before a long break.
    pub cycles_before_long_break: u32,
}

/// A focus (Pomodoro) session.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockFocusSession {
    /// UUID string.
    pub id: String,
    /// Short workspace-local identifier.
    pub sid: u32,
    /// Session start timestamp (RFC 3339).
    pub started_at: String,
    /// Session end timestamp (RFC 3339).
    pub ended_at: Option<String>,
    /// Linked task UUID string.
    pub task_id: Option<String>,
    /// Linked project UUID string.
    pub project_id: Option<String>,
    /// Total planned work cycles.
    pub planned_cycles: u32,
    /// Number of completed work cycles.
    pub completed_cycles: u32,
    /// Current state.
    pub state: TockFocusState,
    /// Configuration snapshot.
    pub config: TockFocusConfig,
    /// Creation timestamp (RFC 3339).
    pub created_at: String,
    /// Last modification timestamp (RFC 3339).
    pub modified_at: String,
}

/// A habit.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockHabit {
    /// UUID string.
    pub id: String,
    /// Short workspace-local identifier.
    pub sid: u32,
    /// Habit title.
    pub title: String,
    /// Identity statement.
    pub identity: Option<String>,
    /// Cue / implementation intention.
    pub cue: Option<String>,
    /// Craving / motivation framing.
    pub craving: Option<String>,
    /// Response / action definition.
    pub response: Option<String>,
    /// Reward after completion.
    pub reward: Option<String>,
    /// Build or break direction.
    pub direction: TockHabitDirection,
    /// Cadence payload (JSON).
    pub cadence: String,
    /// Minimum threshold payload (JSON).
    pub minimum: String,
    /// Parent habit UUID for stacking.
    pub stack_after: Option<String>,
    /// Stack delay in seconds.
    pub stack_delay_s: u32,
    /// Area UUID string.
    pub area_id: Option<String>,
    /// Project UUID string.
    pub project_id: Option<String>,
    /// Current progression level.
    pub level: u32,
    /// Accumulated XP.
    pub xp: u32,
    /// Current streak in cadence periods.
    pub streak_current: u32,
    /// Best streak ever.
    pub streak_best: u32,
    /// Level display name.
    pub level_name: String,
    /// Creation timestamp (RFC 3339).
    pub created_at: String,
    /// Last modification timestamp (RFC 3339).
    pub modified_at: String,
    /// Archive timestamp (RFC 3339).
    pub archived_at: Option<String>,
}

/// A habit log entry.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockHabitEntry {
    /// UUID string.
    pub id: String,
    /// Linked habit UUID string.
    pub habit_id: String,
    /// When the entry occurred (RFC 3339).
    pub occurred_at: String,
    /// Amount payload (JSON).
    pub amount: String,
    /// Optional freeform notes.
    pub notes: Option<String>,
    /// Whether this records a slip for a break habit.
    pub slip: bool,
    /// Source of the log entry.
    pub source: String,
    /// Creation timestamp (RFC 3339).
    pub created_at: String,
}

/// Local sync/device metadata needed by platform transports.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockSyncDeviceInfo {
    /// Shared vault UUID.
    pub vault_id: String,
    /// This device's random 16-byte id as lowercase hex.
    pub device_id: String,
    /// This device's Ed25519 verifying key as lowercase hex.
    pub verifying_key: String,
    /// Human-readable device label, if set.
    pub device_label: Option<String>,
    /// Persisted sync server base URL, if configured.
    pub server_url: Option<String>,
    /// Last consumed server pull cursor.
    pub pull_cursor: u64,
}

/// A single outbound sync frame ready for transport.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockSyncEventFrame {
    /// Event UUID.
    pub event_id: String,
    /// Origin device id as lowercase hex.
    pub device_id: String,
    /// Lamport timestamp.
    pub lamport: u64,
    /// Full wire-format frame bytes for this event.
    pub payload: Vec<u8>,
}

/// Result of ingesting remote sync frames.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockSyncIngestSummary {
    /// Number of foreign events applied locally.
    pub applied: u32,
    /// Number of conflicts recorded for review.
    pub conflicts: u32,
}

/// A pending sync conflict surfaced for user review.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockSyncConflict {
    /// Conflict UUID.
    pub id: String,
    /// Entity kind (`task`, `habit`, ...).
    pub entity_kind: String,
    /// Entity UUID.
    pub entity_id: String,
    /// Human-readable conflict detail.
    pub detail: String,
}

/// Pairing invite details to encode as QR/text on the existing device.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockPairingInvite {
    /// Shared vault UUID.
    pub vault_id: String,
    /// Sync server URL for the vault.
    pub server_url: String,
    /// Inviter ephemeral X25519 public key as lowercase hex.
    pub inviter_pubkey: String,
    /// Inviter fingerprint as lowercase hex.
    pub inviter_fingerprint: String,
    /// When the invite was created (RFC 3339).
    pub created_at: String,
}

/// Acceptor-side values that must be relayed back to the inviter.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockPairingAcceptorInfo {
    /// Acceptor ephemeral X25519 public key as lowercase hex.
    pub accepter_pubkey: String,
    /// Acceptor fingerprint as lowercase hex.
    pub accepter_fingerprint: String,
    /// Rendezvous device id used to fetch the onboarding blob.
    pub rendezvous_device_id: String,
}

// ── Input records ────────────────────────────────────────────────────

/// Input for creating a new task.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockNewTask {
    /// Title (required).
    pub title: String,
    /// Notes.
    pub notes: Option<String>,
    /// Initial status (defaults to Inbox).
    pub status: Option<TockTaskStatus>,
    /// Project UUID to assign to.
    pub project_id: Option<String>,
    /// Area UUID to assign to.
    pub area_id: Option<String>,
    /// Heading UUID within the project.
    pub heading_id: Option<String>,
    /// Deferred start date.
    pub start_date: Option<String>,
    /// Deadline.
    pub deadline: Option<String>,
    /// Recurrence specification (JSON).
    pub recurrence: Option<String>,
    /// Priority.
    pub priority: Option<TockPriority>,
    /// Evening flag.
    pub evening: bool,
    /// Initial UDA values (JSON object string).
    pub udas: String,
    /// Tags to apply.
    pub tags: Vec<String>,
}

/// Patch for modifying an existing task.
///
/// Each optional field means "don't change" when `None` and "set" when
/// `Some`. For fields that can be cleared (set to null), a companion
/// `clear_*` flag is provided. When the clear flag is `true` the field
/// is removed regardless of the value in the corresponding field.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockTaskPatch {
    /// New title.
    pub title: Option<String>,
    /// New notes.
    pub notes: Option<String>,
    /// Clear notes to null.
    pub clear_notes: bool,
    /// New status.
    pub status: Option<TockTaskStatus>,
    /// New project UUID.
    pub project_id: Option<String>,
    /// Clear project assignment.
    pub clear_project: bool,
    /// New area UUID.
    pub area_id: Option<String>,
    /// Clear area assignment.
    pub clear_area: bool,
    /// New heading UUID.
    pub heading_id: Option<String>,
    /// Clear heading assignment.
    pub clear_heading: bool,
    /// New start date.
    pub start_date: Option<String>,
    /// Clear start date.
    pub clear_start_date: bool,
    /// New deadline.
    pub deadline: Option<String>,
    /// Clear deadline.
    pub clear_deadline: bool,
    /// New priority.
    pub priority: Option<TockPriority>,
    /// Clear priority.
    pub clear_priority: bool,
    /// New evening flag.
    pub evening: Option<bool>,
    /// UDA values to set (JSON object string).
    pub set_udas: String,
    /// UDA keys to remove.
    pub remove_uda_keys: Vec<String>,
    /// Tags to add.
    pub add_tags: Vec<String>,
    /// Tags to remove.
    pub remove_tags: Vec<String>,
    /// Dependency SIDs to add.
    pub add_deps: Vec<u32>,
    /// Dependency SIDs to remove.
    pub remove_deps: Vec<u32>,
}

/// Input for creating a new project.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockNewProject {
    /// Project name (required).
    pub name: String,
    /// Notes.
    pub notes: Option<String>,
    /// Area UUID.
    pub area_id: Option<String>,
    /// Deadline.
    pub deadline: Option<String>,
}

/// Input for creating a new area.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockNewArea {
    /// Area name (required).
    pub name: String,
    /// Display color.
    pub color: Option<String>,
}

/// Input for creating a new time block (starts a running timer).
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockNewTimeBlock {
    /// Block title / description.
    pub title: String,
    /// Linked task SID (resolved to UUID internally).
    pub task_sid: Option<u32>,
    /// Linked project UUID.
    pub project_id: Option<String>,
    /// Free-form notes.
    pub notes: Option<String>,
}

/// Input for starting a new focus session.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockNewFocusSession {
    /// Linked task SID (resolved to UUID internally).
    pub task_sid: Option<u32>,
    /// Linked project UUID.
    pub project_id: Option<String>,
    /// Total number of planned work cycles.
    pub planned_cycles: u32,
    /// Session configuration.
    pub config: TockFocusConfig,
}

/// Input for creating a new habit.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TockNewHabit {
    /// Habit title (required).
    pub title: String,
    /// Identity statement.
    pub identity: Option<String>,
    /// Cue / implementation intention.
    pub cue: Option<String>,
    /// Craving / motivation framing.
    pub craving: Option<String>,
    /// Response / action definition.
    pub response: Option<String>,
    /// Reward after completion.
    pub reward: Option<String>,
    /// Build or break direction.
    pub direction: TockHabitDirection,
    /// Cadence payload (JSON).
    pub cadence: String,
    /// Minimum threshold payload (JSON).
    pub minimum: String,
    /// Parent habit SID for stacking.
    pub stack_after: Option<u32>,
    /// Stack delay in seconds.
    pub stack_delay_s: u32,
    /// Area UUID.
    pub area_id: Option<String>,
    /// Project UUID.
    pub project_id: Option<String>,
}
