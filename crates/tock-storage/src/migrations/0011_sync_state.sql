-- Sync state for event-sourced multi-device sync (issue #120).
--
-- The CLI synthesizes signed events by diffing the current domain state
-- against `sync_journal` at sync time, pushes them to a tock-server, and
-- ingests remote events back into the domain tables. These tables hold
-- the bookkeeping for that process. None of them contain plaintext user
-- data beyond what the (already-present) domain tables hold; the wire
-- payloads pushed to the server are AEAD-encrypted event frames.

-- Key/value bag for scalar sync configuration and cursors:
--   'server_url'    -> UTF-8 server base URL
--   'device_label'  -> UTF-8 human label for this device
--   'pull_cursor'   -> little-endian u64 highest server lamport pulled
--   'vector_clock'  -> canonical vector-clock serialization (local clock)
CREATE TABLE sync_state (
    key   TEXT PRIMARY KEY,
    value BLOB NOT NULL
);

-- Last-emitted snapshot per entity, so the outbound collector can detect
-- creates / updates / deletes by diffing current state against this. The
-- full canonical content frame is stored (not just a hash) so the
-- collector can compute changed-field lists for `Update` events and
-- reconstruct key columns for `Delete` events.
CREATE TABLE sync_journal (
    entity_kind  TEXT NOT NULL,
    entity_id    BLOB NOT NULL,         -- 16-byte sync id
    content      BLOB NOT NULL,         -- canonical row snapshot frame
    lamport      INTEGER NOT NULL,      -- lamport of the last emitted event
    PRIMARY KEY (entity_kind, entity_id)
);

-- Current head event ids per entity (the leaves of the per-entity event
-- DAG). Used to classify incoming events for conflict detection.
CREATE TABLE entity_heads (
    entity_kind   TEXT NOT NULL,
    entity_id     BLOB NOT NULL,        -- 16-byte sync id
    head_event_id BLOB NOT NULL,        -- 16-byte event id
    PRIMARY KEY (entity_kind, entity_id, head_event_id)
);
CREATE INDEX entity_heads_entity_idx ON entity_heads (entity_kind, entity_id);

-- Unresolved sync conflicts surfaced for user review (no silent
-- last-write-wins per ADR-003).
CREATE TABLE sync_conflicts (
    id          BLOB PRIMARY KEY,       -- 16-byte uuid
    entity_kind TEXT NOT NULL,
    entity_id   BLOB NOT NULL,
    detail      TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    resolved    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX sync_conflicts_open_idx ON sync_conflicts (resolved);
