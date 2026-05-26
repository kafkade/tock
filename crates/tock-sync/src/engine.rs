//! Sync engine — processes incoming events against local state.
//!
//! The engine is **stateless**: callers provide the incoming event and
//! the current head events for the affected entity, and receive an
//! [`IngestResult`] describing what to do. The storage layer is
//! responsible for persisting the result.
//!
//! ## Workflow
//!
//! ```text
//! pull events from transport
//!   → for each incoming event:
//!       1. classify_against_heads(incoming, entity_heads)
//!       2. if should_apply → Apply
//!       3. if stale → Discard
//!       4. if concurrent → resolve conflicts per policy
//!       5. merge device clock
//!       6. return IngestResult
//! ```

use tock_core::event::{Event, VectorClock};
use uuid::Uuid;

use crate::conflict::{
    self, ConflictEntry, ConflictPolicy, ConflictResolution, HeadClassification,
};

/// What the caller should do with the incoming event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IngestAction {
    /// Apply the event: it supersedes all heads.
    Apply,
    /// Discard the event: it is stale or a duplicate.
    Discard,
    /// Apply the event with field-level merge against the specified
    /// concurrent head. Both the incoming event and the head's fields
    /// are valid and should be materialized.
    MergeFields {
        /// The concurrent head event id that was merged with.
        merged_with: Uuid,
    },
    /// Apply the event, but log the conflict for user review.
    ApplyWithConflictLog,
}

/// Result of processing a single incoming event.
#[derive(Clone, Debug)]
pub struct IngestResult {
    /// What the caller should do.
    pub action: IngestAction,
    /// Updated head set for the entity. The caller must replace the
    /// entity's head events with this set.
    pub new_heads: Vec<Uuid>,
    /// Conflict log entries to persist (may be empty).
    pub conflicts: Vec<ConflictEntry>,
    /// The merged device clock after incorporating the incoming
    /// event's vector clock.
    pub merged_clock: VectorClock,
}

/// Process a single incoming event against the entity's current heads.
///
/// This is the core sync-engine entry point. It:
/// 1. Classifies the event against all heads (transactionally).
/// 2. For concurrent conflicts, applies resolution rules per `policy`.
/// 3. Returns an [`IngestResult`] describing what the caller should
///    persist.
///
/// ## Arguments
///
/// - `incoming`: the event received from a remote device.
/// - `heads`: the current head events for `incoming.entity_id`. Pass
///   an empty slice for a brand-new entity.
/// - `head_events`: the full `Event` objects for each head (needed for
///   conflict resolution). Must be in the same order as `heads`.
/// - `local_clock`: the local device's current vector clock (merged
///   with the incoming event's clock on success).
/// - `policy`: conflict resolution configuration.
#[must_use]
pub fn process_incoming_event(
    incoming: &Event,
    head_events: &[Event],
    local_clock: &VectorClock,
    policy: ConflictPolicy,
) -> IngestResult {
    let classification = conflict::classify_against_heads(incoming, head_events);

    // Duplicate: discard.
    if classification.is_duplicate {
        return IngestResult {
            action: IngestAction::Discard,
            new_heads: head_events.iter().map(|e| e.id).collect(),
            conflicts: vec![],
            merged_clock: local_clock.clone(),
        };
    }

    // Stale: discard.
    if classification.is_stale() {
        return IngestResult {
            action: IngestAction::Discard,
            new_heads: head_events.iter().map(|e| e.id).collect(),
            conflicts: vec![],
            merged_clock: local_clock.clone(),
        };
    }

    // Merge the clocks optimistically.
    let mut merged_clock = local_clock.clone();
    merged_clock.merge(&incoming.vector_clock);

    // Pure supersede: apply, remove superseded heads, add incoming.
    if classification.should_apply() {
        let new_heads = compute_new_heads(&classification, head_events, incoming);
        return IngestResult {
            action: IngestAction::Apply,
            new_heads,
            conflicts: vec![],
            merged_clock,
        };
    }

    // Concurrent: resolve each conflict.
    resolve_concurrent_heads(
        incoming,
        head_events,
        &classification,
        &merged_clock,
        policy,
    )
}

fn compute_new_heads(
    classification: &HeadClassification,
    head_events: &[Event],
    incoming: &Event,
) -> Vec<Uuid> {
    let mut new_heads: Vec<Uuid> = head_events
        .iter()
        .filter(|h| !classification.superseded.contains(&h.id))
        .map(|h| h.id)
        .collect();
    new_heads.push(incoming.id);
    new_heads
}

fn resolve_concurrent_heads(
    incoming: &Event,
    head_events: &[Event],
    classification: &HeadClassification,
    merged_clock: &VectorClock,
    policy: ConflictPolicy,
) -> IngestResult {
    let mut all_conflicts = Vec::new();
    let mut action = IngestAction::Apply;

    // Process conflicts with each concurrent head.
    for concurrent_head_id in &classification.concurrent_with {
        let Some(head_event) = head_events.iter().find(|h| h.id == *concurrent_head_id) else {
            continue;
        };
        let (resolution, entry) = conflict::resolve_concurrent(incoming, head_event, policy);
        if let Some(e) = entry {
            all_conflicts.push(e);
        }
        match resolution {
            ConflictResolution::FieldMerge { .. } => {
                action = IngestAction::MergeFields {
                    merged_with: *concurrent_head_id,
                };
            }
            ConflictResolution::BothApply | ConflictResolution::Idempotent => {
                // Keep Apply.
            }
            ConflictResolution::LastWriterWins { .. }
            | ConflictResolution::DeleteWins { .. }
            | ConflictResolution::UpdateWins { .. } => {
                action = IngestAction::ApplyWithConflictLog;
            }
        }
    }

    // Build new heads: remove superseded, add incoming.
    let mut new_heads: Vec<Uuid> = head_events
        .iter()
        .filter(|h| !classification.superseded.contains(&h.id))
        .map(|h| h.id)
        .collect();
    new_heads.push(incoming.id);

    IngestResult {
        action,
        new_heads,
        conflicts: all_conflicts,
        merged_clock: merged_clock.clone(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use tock_core::event::{DeviceId, EntityKind, EventOp};

    fn dev_a() -> DeviceId {
        DeviceId([1; 16])
    }
    fn dev_b() -> DeviceId {
        DeviceId([2; 16])
    }

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
            created_at: time::OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("ts"),
        }
    }

    #[test]
    fn new_entity_first_event_applies() {
        let vc = VectorClock::singleton(dev_a(), 1);
        let incoming = make_event(dev_a(), 1, vc, EventOp::Create);
        let local_clock = VectorClock::new();
        let result =
            process_incoming_event(&incoming, &[], &local_clock, ConflictPolicy::default());
        assert_eq!(result.action, IngestAction::Apply);
        assert_eq!(result.new_heads, vec![incoming.id]);
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn supersede_single_head() {
        let vc1 = VectorClock::singleton(dev_a(), 1);
        let mut vc2 = VectorClock::singleton(dev_a(), 2);
        vc2.merge(&vc1);
        let head = make_event(dev_a(), 1, vc1.clone(), EventOp::Create);
        let incoming = make_event(
            dev_a(),
            2,
            vc2,
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let result = process_incoming_event(&incoming, &[head], &vc1, ConflictPolicy::default());
        assert_eq!(result.action, IngestAction::Apply);
        assert_eq!(result.new_heads, vec![incoming.id]);
    }

    #[test]
    fn stale_event_discarded() {
        let vc1 = VectorClock::singleton(dev_a(), 1);
        let mut vc2 = VectorClock::singleton(dev_a(), 2);
        vc2.merge(&vc1);
        let head = make_event(
            dev_a(),
            2,
            vc2.clone(),
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let incoming = make_event(dev_a(), 1, vc1, EventOp::Create);
        let result =
            process_incoming_event(&incoming, &[head.clone()], &vc2, ConflictPolicy::default());
        assert_eq!(result.action, IngestAction::Discard);
        assert_eq!(result.new_heads, vec![head.id]);
    }

    #[test]
    fn concurrent_disjoint_updates_merge() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 1);
        let head = make_event(
            dev_a(),
            1,
            vc_a.clone(),
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let incoming = make_event(
            dev_b(),
            1,
            vc_b,
            EventOp::Update {
                fields: vec!["notes".into()],
            },
        );
        let result = process_incoming_event(&incoming, &[head], &vc_a, ConflictPolicy::default());
        assert!(
            matches!(result.action, IngestAction::MergeFields { .. }),
            "disjoint updates should merge"
        );
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn concurrent_overlapping_updates_lww_with_conflict_log() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 2);
        let head = make_event(
            dev_a(),
            1,
            vc_a.clone(),
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let incoming = make_event(
            dev_b(),
            2,
            vc_b,
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let result = process_incoming_event(&incoming, &[head], &vc_a, ConflictPolicy::default());
        assert_eq!(result.action, IngestAction::ApplyWithConflictLog);
        assert_eq!(result.conflicts.len(), 1);
    }

    #[test]
    fn concurrent_update_vs_delete() {
        let vc_a = VectorClock::singleton(dev_a(), 1);
        let vc_b = VectorClock::singleton(dev_b(), 1);
        let head = make_event(
            dev_a(),
            1,
            vc_a.clone(),
            EventOp::Update {
                fields: vec!["title".into()],
            },
        );
        let incoming = make_event(dev_b(), 1, vc_b, EventOp::Delete);
        let result = process_incoming_event(&incoming, &[head], &vc_a, ConflictPolicy::default());
        assert_eq!(result.action, IngestAction::ApplyWithConflictLog);
        assert_eq!(result.conflicts.len(), 1);
    }

    #[test]
    fn clock_merges_on_apply() {
        let vc_a = VectorClock::singleton(dev_a(), 3);
        let mut incoming_vc = VectorClock::singleton(dev_b(), 5);
        incoming_vc.merge(&VectorClock::singleton(dev_a(), 1));
        let incoming = make_event(dev_b(), 5, incoming_vc, EventOp::Create);
        let result = process_incoming_event(&incoming, &[], &vc_a, ConflictPolicy::default());
        assert_eq!(result.action, IngestAction::Apply);
        assert_eq!(result.merged_clock.0.get(&dev_a()).copied(), Some(3));
        assert_eq!(result.merged_clock.0.get(&dev_b()).copied(), Some(5));
    }

    #[test]
    fn duplicate_event_discarded() {
        let vc = VectorClock::singleton(dev_a(), 1);
        let event = make_event(dev_a(), 1, vc.clone(), EventOp::Create);
        let result =
            process_incoming_event(&event, &[event.clone()], &vc, ConflictPolicy::default());
        assert_eq!(result.action, IngestAction::Discard);
    }
}
