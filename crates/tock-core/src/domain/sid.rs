//! Short ID (SID) — workspace-local numeric identifiers.
//!
//! SIDs provide ergonomic CLI references (`42` instead of a UUID).
//! Each entity kind (`task`, `project`, `habit`, etc.) has an
//! independent monotonic counter. SIDs are allocated by the storage
//! layer and recycled after logbook purge.

/// The entity kinds that carry SIDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidKind {
    /// Task SID.
    Task,
    /// Project SID.
    Project,
    /// Habit SID.
    Habit,
    /// Time block SID.
    Block,
    /// Focus session SID.
    Focus,
}

impl SidKind {
    /// Canonical storage key.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Project => "project",
            Self::Habit => "habit",
            Self::Block => "block",
            Self::Focus => "focus",
        }
    }
}
