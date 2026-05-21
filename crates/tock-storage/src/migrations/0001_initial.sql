-- Initial migration: vault metadata, schema-migration tracking, device registry.

CREATE TABLE vault_meta (
    key   TEXT PRIMARY KEY,
    value BLOB NOT NULL
);

CREATE TABLE schema_migrations (
    version    INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    applied_at TEXT NOT NULL,
    checksum   TEXT NOT NULL
);

-- Device registry — maps a 16-byte device id to the Ed25519 verifying
-- key that device uses to sign events. Event verification consults
-- this table and rejects signers not in the registry.
CREATE TABLE devices (
    device_id     BLOB PRIMARY KEY,                   -- 16 bytes
    verifying_key BLOB NOT NULL UNIQUE,               -- 32 bytes (Ed25519 public)
    label         TEXT,
    registered_at TEXT NOT NULL
);
