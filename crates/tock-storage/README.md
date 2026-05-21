# tock-storage

`SQLite` storage adapter for tock. Implements the on-disk vault format
and the append-only event log on top of `rusqlite` (with `bundled`
SQLite). Sensitive data — currently event payloads — is AEAD-encrypted
at the application layer via [`tock-crypto`](../tock-crypto/).

## Phase 0 storage layout

The vault is a single `.tockvault` SQLite file. The vault header (magic,
format version, salts, Argon2 parameters, wrapped Vault Key) lives in a
`vault_meta` table; every metadata field that participates in key
derivation is bound into the AES-GCM AAD when the Vault Key is wrapped,
so any tampering with the header invalidates the wrap. Event payloads
are encrypted with item keys derived from the Vault Key via HKDF and
signed with the device's Ed25519 key.

The on-disk format records `storage_layout = "sqlite-plain-app-aead-v0"`
in the header so a future `SQLCipher` integration can detect and
upgrade it.

Licensed under [Apache-2.0](../../LICENSE-APACHE). See
[ADR-002](../../docs/adr/ADR-002-end-to-end-encryption.md),
[ADR-003](../../docs/adr/ADR-003-event-sourced-sync.md), and
[ADR-004](../../docs/adr/ADR-004-sqlite-app-layer-encryption.md).
