//! Heading domain model per architecture §2.1.1.
//!
//! Headings are pure presentational grouping inside a project. No
//! state, no dates, just an ordered label. Tasks can belong to a
//! heading or be top-level in a project.

use time::OffsetDateTime;
use uuid::Uuid;

/// A heading within a project.
#[derive(Clone, Debug)]
pub struct Heading {
    /// Globally unique identifier.
    pub id: Uuid,
    /// The project this heading belongs to.
    pub project_id: Uuid,
    /// Heading text.
    pub name: String,
    /// Sort position within the project.
    pub sort_order: i32,
    /// When created.
    pub created_at: OffsetDateTime,
    /// When last modified.
    pub modified_at: OffsetDateTime,
}

/// Input for creating a new heading.
#[derive(Clone, Debug)]
pub struct NewHeading {
    /// Project this heading belongs to.
    pub project_id: Uuid,
    /// Heading text.
    pub name: String,
}
