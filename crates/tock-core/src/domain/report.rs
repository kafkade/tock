//! Custom report definitions.

use time::OffsetDateTime;
use uuid::Uuid;

/// A saved report definition.
#[derive(Clone, Debug)]
pub struct Report {
    /// Globally unique identifier for the report.
    pub id: Uuid,
    /// User-visible report name.
    pub name: String,
    /// Filter expression in the query DSL.
    pub query: String,
    /// Sort field to apply when showing the report.
    pub sort: Option<String>,
    /// Column names to display in table output.
    pub columns: Vec<String>,
    /// When the report was created.
    pub created_at: OffsetDateTime,
    /// When the report was last modified.
    pub modified_at: OffsetDateTime,
}

/// Input for creating a report.
#[derive(Clone, Debug)]
pub struct NewReport {
    /// User-visible report name.
    pub name: String,
    /// Filter expression in the query DSL.
    pub query: String,
    /// Sort field to apply when showing the report.
    pub sort: Option<String>,
    /// Column names to display in table output.
    pub columns: Vec<String>,
}
