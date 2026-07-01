//! Annotation domain model per architecture §2.1.8.
//!
//! Annotations are append-only, timestamped free-form notes attached to
//! an entity (currently only tasks). Unlike a task's single `notes`
//! field, a task can accumulate many annotations over time, each with
//! its own creation timestamp — mirroring Taskwarrior's `annotate`.
//!
//! Storage lives in the `annotations` table (join by `entity_id` +
//! `entity_kind`); this module only defines the value types.

use time::OffsetDateTime;
use uuid::Uuid;

/// The kind of entity an annotation is attached to.
///
/// The storage layer constrains `entity_kind` to
/// `('task','project','habit','block')`; the current feature set only
/// annotates tasks.
pub const ENTITY_KIND_TASK: &str = "task";

/// A stored annotation — an append-only note on an entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Annotation {
    /// Globally unique identifier (`UUIDv7`).
    pub id: Uuid,
    /// The entity this annotation is attached to.
    pub entity_id: Uuid,
    /// The kind of entity (e.g. `task`).
    pub entity_kind: String,
    /// Free-form annotation text.
    pub body: String,
    /// When the annotation was created.
    pub created_at: OffsetDateTime,
}

/// Input for creating a new annotation.
#[derive(Clone, Debug)]
pub struct NewAnnotation {
    /// The entity to attach to.
    pub entity_id: Uuid,
    /// The kind of entity (e.g. `task`).
    pub entity_kind: String,
    /// Annotation text.
    pub body: String,
    /// Explicit creation timestamp (defaults to "now" when `None`).
    pub created_at: Option<OffsetDateTime>,
}

impl NewAnnotation {
    /// Build a new task annotation with the given body.
    #[must_use]
    pub fn for_task(entity_id: Uuid, body: impl Into<String>) -> Self {
        Self {
            entity_id,
            entity_kind: ENTITY_KIND_TASK.to_string(),
            body: body.into(),
            created_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_task_sets_task_kind() {
        let id = Uuid::now_v7();
        let a = NewAnnotation::for_task(id, "reviewed with team");
        assert_eq!(a.entity_id, id);
        assert_eq!(a.entity_kind, ENTITY_KIND_TASK);
        assert_eq!(a.body, "reviewed with team");
        assert!(a.created_at.is_none());
    }
}
