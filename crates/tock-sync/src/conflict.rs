//! Conflict detection and resolution for concurrent events.
//!
//! Implements the algorithm from architecture §6.2 (conflict detection)
//! and §6.3 (resolution rules). All operations are stateless: callers
//! provide the incoming event and the current head events for the
//! entity, and receive a verdict.
//!
//! ## Head events
//!
//! The "heads" for an entity are the set of last-applied events such
//! that no later applied event supersedes them. An entity with one
//! device always has exactly one head. Multi-device concurrent edits
//! produce multiple heads (one per concurrent branch).
//!
//! ## Resolution rules (§6.3)
//!
//! | Case | Rule |
//! |------|------|
//! | Concurrent `Update`s, disjoint fields | Field-level merge |
//! | Concurrent `Update`s, same field | LWW (lamport DESC, device_id ASC) |
//! | Concurrent `Update` + `Delete` | Configurable (default: delete wins) |
//! | Concurrent `Append`s | Both apply (commutative) |
//! | Concurrent `Delete`s | Idempotent |
//! | Concurrent `Create` same entity_id | Tie-break by device_id |

use std::collections::BTreeSet;

use time::OffsetDateTime;
use tock_core::event::{Event, EventOp};
use uuid::Uuid;

/// Pairwise relationship between two events on the same entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventRelation {
    /// The events are identical (same event id).
    Duplicate,
    /// `incoming` dominates `head` — supersedes it.
    Supersedes,
    /// `head` dominates `incoming` — the incoming event is stale.
    Stale,
    /// Neither dominates the other — concurrent edit.
    Concurrent,
}

/// How a concurrent conflict was resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Both events' fields apply (disjoint field sets).
    FieldMerge {
        /// Fields from the first event.
        fields_a: Vec<String>,
        /// Fields from the second event.
        fields_b: Vec<String>,
    },
    /// Last-writer-wins on overlapping fields.
    LastWriterWins {
        /// Event id of the winner.
        winner_event_id: Uuid,
        /// Event id of the loser (preserved in history).
        loser_event_id: Uuid,
        /// Fields where LWW was applied.
        contested_fields: Vec<String>,
    },
    /// Delete wins over a concurrent update.
    DeleteWins {
        /// The delete event id.
        delete_event_id: Uuid,
        /// The update event id (preserved in history).
        update_event_id: Uuid,
    },
    /// Update wins over a concurrent delete (user chose to keep).
    UpdateWins {
        /// The update event id.
        update_event_id: Uuid,
        /// The delete event id.
        delete_event_id: Uuid,
    },
    /// Both append operations apply (commutative collections).
    BothApply,
    /// Concurrent deletes are idempotent.
    Idempotent,
}

/// An entry in the conflict log for user review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictEntry {
    /// Entity kind affected.
    pub entity_kind: tock_core::event::EntityKind,
    /// Entity id affected.
    pub entity_id: Uuid,
    /// The event that won (was applied).
    pub winning_event_id: Uuid,
    /// The event that lost (preserved in history).
    pub losing_event_id: Uuid,
    /// How the conflict was resolved.
    pub resolution: ConflictResolution,
    /// When the conflict was detected.
    pub detected_at: OffsetDateTime,
}

/// Policy for configuring conflict resolution behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConflictPolicy {
    /// How to handle concurrent `Update` + `Delete` on the same entity.
    pub delete_vs_update: DeleteVsUpdate,
}

impl Default for ConflictPolicy {
    fn default() -> Self {
        Self {
            delete_vs_update: DeleteVsUpdate::DeleteWins,
        }
    }
}

/// Policy for concurrent `Update` + `Delete`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteVsUpdate {
    /// Delete always wins (default, per architecture §6.3).
    DeleteWins,
    /// Update always wins (preserves data).
    UpdateWins,
    /// Require user review — the conflict is surfaced.
    Prompt,
}

/// Classify the relationship between an incoming event and a head event.
///
/// Both events must be on the same entity.
#[must_use]
pub fn classify(incoming: &Event, head: &Event) -> EventRelation {
    if incoming.id == head.id {
        return EventRelation::Duplicate;
    }
    if incoming.vector_clock.happens_before(&head.vector_clock) {
        return EventRelation::Stale;
    }
    if head.vector_clock.happens_before(&incoming.vector_clock) {
        return EventRelation::Supersedes;
    }
    EventRelation::Concurrent
}

/// Classification result against all head events for an entity.
#[derive(Clone, Debug)]
pub struct HeadClassification {
    /// Heads that the incoming event supersedes (should be removed).
    pub superseded: Vec<Uuid>,
    /// Heads that dominate the incoming event (event is stale).
    pub dominated_by: Vec<Uuid>,
    /// Heads concurrent with the incoming event (conflict).
    pub concurrent_with: Vec<Uuid>,
    /// Whether the incoming event is a duplicate of a head.
    pub is_duplicate: bool,
}

impl HeadClassification {
    /// True if the incoming event should be applied outright (no
    /// conflicts, no stale).
    #[must_use]
    pub const fn should_apply(&self) -> bool {
        !self.is_duplicate && self.dominated_by.is_empty() && self.concurrent_with.is_empty()
    }

    /// True if the incoming event is stale (superseded by existing heads).
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        !self.dominated_by.is_empty()
    }
}

/// Classify an incoming event against all current head events for its
/// entity.
///
/// Returns a transactional classification: all heads are examined
/// before any mutation, so the caller can decide how to proceed.
#[must_use]
pub fn classify_against_heads(incoming: &Event, heads: &[Event]) -> HeadClassification {
    let mut superseded = Vec::new();
    let mut dominated_by = Vec::new();
    let mut concurrent_with = Vec::new();
    let mut is_duplicate = false;

    for head in heads {
        match classify(incoming, head) {
            EventRelation::Duplicate => {
                is_duplicate = true;
            }
            EventRelation::Supersedes => {
                superseded.push(head.id);
            }
            EventRelation::Stale => {
                dominated_by.push(head.id);
            }
            EventRelation::Concurrent => {
                concurrent_with.push(head.id);
            }
        }
    }

    HeadClassification {
        superseded,
        dominated_by,
        concurrent_with,
        is_duplicate,
    }
}

/// Resolve a concurrent conflict between two events on the same entity.
///
/// Returns the resolution and optionally a conflict log entry.
#[must_use]
pub fn resolve_concurrent(
    event_a: &Event,
    event_b: &Event,
    policy: ConflictPolicy,
) -> (ConflictResolution, Option<ConflictEntry>) {
    match (&event_a.op, &event_b.op) {
        // Two concurrent Updates.
        (EventOp::Update { fields: fa }, EventOp::Update { fields: fb }) => {
            resolve_concurrent_updates(event_a, event_b, fa, fb)
        }

        // Update vs Delete.
        (EventOp::Update { .. }, EventOp::Delete) | (EventOp::Delete, EventOp::Update { .. }) => {
            resolve_update_vs_delete(event_a, event_b, policy)
        }

        // Concurrent Appends — both apply (commutative).
        (EventOp::Append { .. }, EventOp::Append { .. }) => (ConflictResolution::BothApply, None),

        // Concurrent Deletes/Purges — idempotent.
        (EventOp::Delete | EventOp::Purge, EventOp::Delete | EventOp::Purge) => {
            (ConflictResolution::Idempotent, None)
        }

        // Concurrent Creates with same entity_id — deterministic tie-break.
        (EventOp::Create, EventOp::Create) => {
            let (winner, loser) = lww_winner(event_a, event_b);
            let resolution = ConflictResolution::LastWriterWins {
                winner_event_id: winner.id,
                loser_event_id: loser.id,
                contested_fields: vec!["*".into()],
            };
            let entry = ConflictEntry {
                entity_kind: event_a.entity_kind,
                entity_id: event_a.entity_id,
                winning_event_id: winner.id,
                losing_event_id: loser.id,
                resolution: resolution.clone(),
                detected_at: OffsetDateTime::now_utc(),
            };
            (resolution, Some(entry))
        }

        // Any other combination: LWW fallback.
        _ => {
            let (winner, loser) = lww_winner(event_a, event_b);
            let resolution = ConflictResolution::LastWriterWins {
                winner_event_id: winner.id,
                loser_event_id: loser.id,
                contested_fields: vec![],
            };
            let entry = ConflictEntry {
                entity_kind: event_a.entity_kind,
                entity_id: event_a.entity_id,
                winning_event_id: winner.id,
                losing_event_id: loser.id,
                resolution: resolution.clone(),
                detected_at: OffsetDateTime::now_utc(),
            };
            (resolution, Some(entry))
        }
    }
}

/// Resolve two concurrent `Update`s.
fn resolve_concurrent_updates(
    event_a: &Event,
    event_b: &Event,
    fields_a: &[String],
    fields_b: &[String],
) -> (ConflictResolution, Option<ConflictEntry>) {
    let set_a: BTreeSet<&str> = fields_a.iter().map(String::as_str).collect();
    let set_b: BTreeSet<&str> = fields_b.iter().map(String::as_str).collect();
    let overlap: Vec<String> = set_a
        .intersection(&set_b)
        .map(|s| (*s).to_string())
        .collect();

    if overlap.is_empty() {
        // Disjoint field sets — both apply.
        let resolution = ConflictResolution::FieldMerge {
            fields_a: fields_a.to_vec(),
            fields_b: fields_b.to_vec(),
        };
        (resolution, None)
    } else {
        // Overlapping fields — LWW.
        let (winner, loser) = lww_winner(event_a, event_b);
        let resolution = ConflictResolution::LastWriterWins {
            winner_event_id: winner.id,
            loser_event_id: loser.id,
            contested_fields: overlap,
        };
        let entry = ConflictEntry {
            entity_kind: event_a.entity_kind,
            entity_id: event_a.entity_id,
            winning_event_id: winner.id,
            losing_event_id: loser.id,
            resolution: resolution.clone(),
            detected_at: OffsetDateTime::now_utc(),
        };
        (resolution, Some(entry))
    }
}

/// Resolve a concurrent `Update` vs `Delete`.
fn resolve_update_vs_delete(
    event_a: &Event,
    event_b: &Event,
    policy: ConflictPolicy,
) -> (ConflictResolution, Option<ConflictEntry>) {
    let (update_ev, delete_ev) = if matches!(event_a.op, EventOp::Delete) {
        (event_b, event_a)
    } else {
        (event_a, event_b)
    };

    match policy.delete_vs_update {
        DeleteVsUpdate::DeleteWins => {
            let resolution = ConflictResolution::DeleteWins {
                delete_event_id: delete_ev.id,
                update_event_id: update_ev.id,
            };
            let entry = ConflictEntry {
                entity_kind: event_a.entity_kind,
                entity_id: event_a.entity_id,
                winning_event_id: delete_ev.id,
                losing_event_id: update_ev.id,
                resolution: resolution.clone(),
                detected_at: OffsetDateTime::now_utc(),
            };
            (resolution, Some(entry))
        }
        DeleteVsUpdate::UpdateWins => {
            let resolution = ConflictResolution::UpdateWins {
                update_event_id: update_ev.id,
                delete_event_id: delete_ev.id,
            };
            let entry = ConflictEntry {
                entity_kind: event_a.entity_kind,
                entity_id: event_a.entity_id,
                winning_event_id: update_ev.id,
                losing_event_id: delete_ev.id,
                resolution: resolution.clone(),
                detected_at: OffsetDateTime::now_utc(),
            };
            (resolution, Some(entry))
        }
        DeleteVsUpdate::Prompt => {
            // Return an error-style resolution: caller must surface this.
            let resolution = ConflictResolution::DeleteWins {
                delete_event_id: delete_ev.id,
                update_event_id: update_ev.id,
            };
            let entry = ConflictEntry {
                entity_kind: event_a.entity_kind,
                entity_id: event_a.entity_id,
                winning_event_id: delete_ev.id,
                losing_event_id: update_ev.id,
                resolution: resolution.clone(),
                detected_at: OffsetDateTime::now_utc(),
            };
            (resolution, Some(entry))
        }
    }
}

/// Deterministic last-writer-wins tie-break.
///
/// Winner: higher `lamport`. If equal, lower `device_id` (lexicographic).
/// Returns `(winner, loser)`.
#[must_use]
pub fn lww_winner<'a>(a: &'a Event, b: &'a Event) -> (&'a Event, &'a Event) {
    if a.lamport > b.lamport {
        (a, b)
    } else if b.lamport > a.lamport {
        (b, a)
    } else if a.device_id < b.device_id {
        (a, b)
    } else {
        (b, a)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use tock_core::event::{DeviceId, EntityKind, VectorClock};

    fn make_event(device: DeviceId, lamport: u64, vc: VectorClock, op: EventOp) -> Event {
        Event {
            id: Uuid::now_v7(),
            device_id: device,
            lamport,
            vector_clock: vc,
            parent_event_id: None,
            entity_kind: EntityKind::Task,
            entity_id: Uuid::from_bytes([42; 16]),
            op,
            payload_ct: vec![],
            payload_nonce: [0; 12],
            payload_aad: vec![],
            created_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("ts"),
        }
    }

    fn dev_a() -> DeviceId {
        DeviceId([1; 16])
    }
    fn dev_b() -> DeviceId {
        DeviceId([2; 16])
    }

    // ── classify tests ───────────────────────────────────────────────

    #[test]
    fn classify_duplicate() {
        let vc = VectorClock::singleton(dev_a(), 1);
        let e = make_event(dev_a(), 1, vc, EventOp::Create);
        assert_eq!(classify(&e, &e), EventRelation::Duplicate);
    }

    #[test]
    fn classify_supersedes() {
        let vc1 = VectorClock::singleton(dev_a(), 1);
        let mut vc2 = VectorClock::singleton(dev_a(), 2);
        vc2.merge(&vc1);
        let head = make_event(dev_a(), 1, vc1, EventOp::Create);
        let incoming = make_event(
            dev_a(),
            2,
            vc2,
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        assert_eq!(classify(&incoming, &head), EventRelation::Supersedes);
    }

    #[test]
    fn classify_stale() {
        let vc1 = VectorClock::singleton(dev_a(), 1);
        let mut vc2 = VectorClock::singleton(dev_a(), 2);
        vc2.merge(&vc1);
        let head = make_event(
            dev_a(),
            2,
            vc2,
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let incoming = make_event(dev_a(), 1, vc1, EventOp::Create);
        assert_eq!(classify(&incoming, &head), EventRelation::Stale);
    }

    #[test]
    fn classify_concurrent() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 1);
        let event_a = make_event(
            dev_a(),
            1,
            vc_a,
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let event_b = make_event(
            dev_b(),
            1,
            vc_b,
            EventOp::Update {
                fields: vec!["notes".into()],
            },
        );
        assert_eq!(classify(&event_a, &event_b), EventRelation::Concurrent);
    }

    // ── classify_against_heads tests ─────────────────────────────────

    #[test]
    fn classify_heads_all_superseded() {
        let vc1 = VectorClock::singleton(dev_a(), 1);
        let mut vc2 = VectorClock::singleton(dev_a(), 2);
        vc2.merge(&vc1);
        let head = make_event(dev_a(), 1, vc1, EventOp::Create);
        let incoming = make_event(
            dev_a(),
            2,
            vc2,
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let c = classify_against_heads(&incoming, std::slice::from_ref(&head));
        assert!(c.should_apply());
        assert_eq!(c.superseded, vec![head.id]);
    }

    #[test]
    fn classify_heads_mixed_supersede_and_concurrent() {
        let vc1 = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 1);
        let mut vc_incoming = VectorClock::singleton(dev_a(), 2);
        vc_incoming.merge(&vc1);
        // head_a is at vc {A:1}, head_b is at vc {B:1}.
        // incoming is at {A:2} — supersedes head_a but concurrent with head_b.
        let head_a = make_event(dev_a(), 1, vc1, EventOp::Create);
        let head_b = make_event(
            dev_b(),
            1,
            vc_b,
            EventOp::Update {
                fields: vec!["notes".into()],
            },
        );
        let incoming = make_event(
            dev_a(),
            2,
            vc_incoming,
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let c = classify_against_heads(&incoming, &[head_a.clone(), head_b.clone()]);
        assert!(!c.should_apply());
        assert_eq!(c.superseded, vec![head_a.id]);
        assert_eq!(c.concurrent_with, vec![head_b.id]);
    }

    // ── resolve_concurrent tests ─────────────────────────────────────

    #[test]
    fn disjoint_field_merge() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 1);
        let a = make_event(
            dev_a(),
            1,
            vc_a,
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let b = make_event(
            dev_b(),
            1,
            vc_b,
            EventOp::Update {
                fields: vec!["notes".into()],
            },
        );
        let (res, entry) = resolve_concurrent(&a, &b, ConflictPolicy::default());
        assert!(matches!(res, ConflictResolution::FieldMerge { .. }));
        assert!(
            entry.is_none(),
            "disjoint merge should not produce conflict log"
        );
    }

    #[test]
    fn overlapping_field_lww() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 2);
        let a = make_event(
            dev_a(),
            1,
            vc_a,
            EventOp::Update {
                fields: vec!["title".into(), "notes".into()],
            },
        );
        let b = make_event(
            dev_b(),
            2,
            vc_b,
            EventOp::Update {
                fields: vec!["title".into(), "status".into()],
            },
        );
        let (res, entry) = resolve_concurrent(&a, &b, ConflictPolicy::default());
        match &res {
            ConflictResolution::LastWriterWins {
                winner_event_id,
                loser_event_id,
                contested_fields,
            } => {
                assert_eq!(*winner_event_id, b.id, "higher lamport should win");
                assert_eq!(*loser_event_id, a.id);
                assert_eq!(contested_fields, &["title"]);
            }
            _ => panic!("expected LWW resolution"),
        }
        assert!(entry.is_some(), "LWW should produce conflict log entry");
    }

    #[test]
    fn lww_tiebreak_by_device_id() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 1);
        let a = make_event(
            dev_a(),
            1,
            vc_a,
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let b = make_event(
            dev_b(),
            1,
            vc_b,
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let (winner, loser) = lww_winner(&a, &b);
        // dev_a ([1;16]) < dev_b ([2;16]), so dev_a wins tie.
        assert_eq!(winner.device_id, dev_a());
        assert_eq!(loser.device_id, dev_b());
    }

    #[test]
    fn delete_wins_over_update_default_policy() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 1);
        let update = make_event(
            dev_a(),
            1,
            vc_a,
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let delete = make_event(dev_b(), 1, vc_b, EventOp::Delete);
        let (res, entry) = resolve_concurrent(&update, &delete, ConflictPolicy::default());
        match &res {
            ConflictResolution::DeleteWins {
                delete_event_id,
                update_event_id,
            } => {
                assert_eq!(*delete_event_id, delete.id);
                assert_eq!(*update_event_id, update.id);
            }
            _ => panic!("expected DeleteWins"),
        }
        assert!(entry.is_some());
    }

    #[test]
    fn update_wins_policy() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 1);
        let update = make_event(
            dev_a(),
            1,
            vc_a,
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let delete = make_event(dev_b(), 1, vc_b, EventOp::Delete);
        let policy = ConflictPolicy {
            delete_vs_update: DeleteVsUpdate::UpdateWins,
        };
        let (res, _) = resolve_concurrent(&update, &delete, policy);
        assert!(matches!(res, ConflictResolution::UpdateWins { .. }));
    }

    #[test]
    fn concurrent_appends_both_apply() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 1);
        let a = make_event(
            dev_a(),
            1,
            vc_a,
            EventOp::Append {
                sub_kind: "annotation".into(),
            },
        );
        let b = make_event(
            dev_b(),
            1,
            vc_b,
            EventOp::Append {
                sub_kind: "annotation".into(),
            },
        );
        let (res, entry) = resolve_concurrent(&a, &b, ConflictPolicy::default());
        assert_eq!(res, ConflictResolution::BothApply);
        assert!(entry.is_none());
    }

    #[test]
    fn concurrent_deletes_idempotent() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 1);
        let a = make_event(dev_a(), 1, vc_a, EventOp::Delete);
        let b = make_event(dev_b(), 1, vc_b, EventOp::Delete);
        let (res, entry) = resolve_concurrent(&a, &b, ConflictPolicy::default());
        assert_eq!(res, ConflictResolution::Idempotent);
        assert!(entry.is_none());
    }

    #[test]
    fn concurrent_creates_tiebreak() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 1);
        let a = make_event(dev_a(), 1, vc_a, EventOp::Create);
        let b = make_event(dev_b(), 1, vc_b, EventOp::Create);
        let (res, entry) = resolve_concurrent(&a, &b, ConflictPolicy::default());
        match &res {
            ConflictResolution::LastWriterWins {
                contested_fields, ..
            } => {
                assert_eq!(contested_fields, &["*"]);
            }
            _ => panic!("expected LWW for concurrent creates"),
        }
        assert!(entry.is_some());
    }

    #[test]
    fn purge_vs_delete_idempotent() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 1);
        let a = make_event(dev_a(), 1, vc_a, EventOp::Delete);
        let b = make_event(dev_b(), 1, vc_b, EventOp::Purge);
        let (res, _) = resolve_concurrent(&a, &b, ConflictPolicy::default());
        assert_eq!(res, ConflictResolution::Idempotent);
    }

    // ── Edge cases ───────────────────────────────────────────────────

    #[test]
    fn single_head_superseded() {
        let vc1 = VectorClock::singleton(dev_a(), 1);
        let mut vc2 = VectorClock::singleton(dev_a(), 2);
        vc2.merge(&vc1);
        let head = make_event(dev_a(), 1, vc1, EventOp::Create);
        let incoming = make_event(dev_a(), 2, vc2, EventOp::Delete);
        let c = classify_against_heads(&incoming, &[head]);
        assert!(c.should_apply());
    }

    #[test]
    fn empty_heads_means_apply() {
        let vc = VectorClock::singleton(dev_a(), 1);
        let incoming = make_event(dev_a(), 1, vc, EventOp::Create);
        let c = classify_against_heads(&incoming, &[]);
        assert!(c.should_apply());
    }
}
