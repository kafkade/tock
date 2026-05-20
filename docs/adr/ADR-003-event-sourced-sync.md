# ADR-003: Event-sourced sync with vector clocks

**Status:** Accepted  
**Date:** 2026-05-20

## Context

Tock must synchronize across multiple devices (CLI, iOS, iPad, Mac, watchOS) with unreliable connectivity. Traditional timestamp-based sync creates race conditions (last-write-wins loses concurrent edits). Operational-transform (OT) and CRDTs solve concurrency but add complexity or require mergeable data types.

We need conflict detection that is precise (never silently drops changes), resolution that is predictable, and a protocol simple enough to implement correctly across platforms.

## Decision

Tock uses **event-sourced sync with vector clocks**:

**Event structure:**
- UUIDv7 (time-ordered, globally unique)
- Device ID (16-byte random identifier per device)
- Lamport clock (monotonic per device)
- Vector clock (map of device ID → last seen Lamport value)
- Parent event ID (chain integrity)
- Entity kind + entity ID + operation
- Encrypted payload (full state for `Create`, changed fields for `Update`, append data for `Append`)

**Conflict detection:**
For each incoming event `e` on entity `E`, compare `e.vector_clock` against the vector clock of E's current head events:
- If `e` dominates all heads → `e` supersedes them (apply).
- If a head dominates `e` → `e` is stale (discard).
- If concurrent (neither dominates) → **conflict detected**.

**Resolution rules:**
- Disjoint field updates: merge both (field-level granularity).
- Same-field updates: LWW by `(lamport DESC, device_id ASC)`.
- Append-only collections (annotations, habit entries, time blocks, interruptions): apply both.
- `Update` vs. `Delete`: `Delete` wins by default (configurable).
- Open time blocks overlapping across devices: mark both with `+SYNC_OVERLAP` tag.
- Concurrent focus sessions: earlier `started_at` wins; loser marked `Aborted`.

Losing values in LWW conflicts are preserved in the event log and surfaced in `tock sync conflicts` for user review.

**Snapshot compaction:**
Every 1000 events or 30 days, a snapshot event captures full materialized state for each entity, signed and encrypted. Cold sync downloads the latest snapshot, then deltas. This bounds cold-start bandwidth.

**Total order for non-conflicting events:**
Events ordered by `(lamport DESC, device_id ASC)` provide deterministic replay across devices.

## Consequences

**Positive:**
- Deterministic conflict detection (no silent data loss).
- Field-level merge precision (editing `due_date` on device A and `priority` on device B merges cleanly).
- Append-only logs (habits, time blocks) are naturally conflict-free.
- Full audit trail for forensics.

**Negative:**
- Event log grows unbounded without compaction (mitigated by snapshots).
- LWW on same-field conflicts may surprise users (surfaced in conflict log for review).
- Vector clock storage scales with device count (acceptable for personal productivity use case).

**Neutral:**
- No auto-merge heuristics (e.g., "merge tags" or "newest wins"); all rules are explicit and documented.
