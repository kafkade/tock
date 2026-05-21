-- Append-only event log. Per architecture §3.2 and §6.1 (with the
-- `signature` column required by issue #5).
--
-- Each event's payload is AEAD-encrypted using an item key derived
-- from VK + entity_kind + entity_id (see tock-core::vault::KeyHierarchy).

CREATE TABLE events (
    id                BLOB PRIMARY KEY,    -- uuidv7 bytes
    device_id         BLOB NOT NULL,       -- 16 bytes
    lamport           INTEGER NOT NULL,
    vector_clock      BLOB NOT NULL,       -- canonical serialization
    parent_event_id   BLOB,                -- nullable
    entity_kind       TEXT NOT NULL,
    entity_id         BLOB NOT NULL,
    op_tag            TEXT NOT NULL,       -- 'create' | 'update' | ...
    op_sub_tag        BLOB NOT NULL,       -- variant-specific bytes
    payload_ct        BLOB NOT NULL,
    payload_nonce     BLOB NOT NULL,       -- 12 bytes
    payload_aad       BLOB NOT NULL,
    signature         BLOB NOT NULL,       -- 64 bytes Ed25519
    signer            BLOB NOT NULL,       -- 32 bytes Ed25519 verifying key
    created_at        TEXT NOT NULL
);

CREATE INDEX events_entity_idx ON events (entity_kind, entity_id, lamport);
CREATE INDEX events_device_idx ON events (device_id, lamport);
CREATE INDEX events_created_idx ON events (created_at);
