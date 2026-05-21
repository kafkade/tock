//! Area domain model per architecture §2.1.1.
//!
//! Areas are long-lived life domains (e.g. `work`, `health`, `family`).
//! No completion semantics — areas are never "done."

use time::OffsetDateTime;
use uuid::Uuid;

/// An area — long-lived life domain.
#[derive(Clone, Debug)]
pub struct Area {
    /// Globally unique identifier.
    pub id: Uuid,
    /// Area name.
    pub name: String,
    /// Display color.
    pub color: Option<String>,
    /// Sort position.
    pub sort_order: i32,
    /// When created.
    pub created_at: OffsetDateTime,
    /// When last modified.
    pub modified_at: OffsetDateTime,
    /// When archived (if archived).
    pub archived_at: Option<OffsetDateTime>,
}

/// Input for creating a new area.
#[derive(Clone, Debug)]
pub struct NewArea {
    /// Name (required).
    pub name: String,
    /// Display color.
    pub color: Option<String>,
}
