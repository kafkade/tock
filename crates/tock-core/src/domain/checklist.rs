//! Checklist item domain model per architecture §2.1.
//!
//! A checklist item is a lightweight sub-task belonging to a task: it has
//! only a `title` and a done state. Checklist items cannot be scheduled,
//! tagged, or have their own checklist — they model "steps to complete this
//! task", not project decomposition (use sub-projects/headings for that).

use time::OffsetDateTime;
use uuid::Uuid;

/// A single checklist item attached to a task.
#[derive(Clone, Debug)]
pub struct ChecklistItem {
    /// Globally unique identifier (`UUIDv7`).
    pub id: Uuid,
    /// The task this item belongs to.
    pub task_id: Uuid,
    /// Item text.
    pub title: String,
    /// Zero-based position within the task's ordered checklist.
    pub position: u32,
    /// When the item was completed (if done).
    pub done_at: Option<OffsetDateTime>,
    /// When the item was created.
    pub created_at: OffsetDateTime,
}

impl ChecklistItem {
    /// Whether the item is checked off.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.done_at.is_some()
    }
}

/// Progress across a set of checklist items: `(done, total)`.
///
/// Returns `None` when the slice is empty so callers can omit the display
/// entirely for tasks without a checklist.
#[must_use]
pub fn progress(items: &[ChecklistItem]) -> Option<(usize, usize)> {
    if items.is_empty() {
        return None;
    }
    let done = items.iter().filter(|item| item.is_done()).count();
    Some((done, items.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, done: bool) -> ChecklistItem {
        ChecklistItem {
            id: Uuid::nil(),
            task_id: Uuid::nil(),
            title: title.to_string(),
            position: 0,
            done_at: done.then_some(OffsetDateTime::UNIX_EPOCH),
            created_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn done_derives_from_timestamp() {
        assert!(!item("a", false).is_done());
        assert!(item("b", true).is_done());
    }

    #[test]
    fn progress_none_when_empty() {
        assert_eq!(progress(&[]), None);
    }

    #[test]
    fn progress_counts_done() {
        let items = [item("a", true), item("b", false), item("c", true)];
        assert_eq!(progress(&items), Some((2, 3)));
    }
}
