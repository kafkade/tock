# ADR-004: SQLite with app-layer encryption

**Status:** Accepted  
**Date:** 2026-05-20

## Context

Tock needs durable storage for tasks, habits, time blocks, focus sessions, and sync events. The database must be:

1. Encrypted at rest to protect against disk theft.
2. Fast enough for sub-50ms queries on 10,000+ tasks.
3. Portable across platforms (CLI, iOS, WASM).
4. Schema-versioned with safe migrations.
5. Queryable with rich filtering (date ranges, tags, urgency thresholds).

NoSQL stores (e.g., IndexedDB alone) lack expressive query support. Cloud-native databases require network I/O, violating the zero-I/O core contract. File-based encrypted storage (JSON, CBOR) requires full load/save, which is too slow for large datasets.

## Decision

Tock uses **SQLite with app-layer encryption**:

**Storage layer:**
- CLI: `rusqlite` with bundled SQLCipher (AES-256-CBC page-level encryption, keyed by VK).
- WASM: `tock-storage-web` using IndexedDB with the same schema (encrypted blobs).

**Schema strategy:**
- No ORM. Direct SQL with typed Rust structs via `rusqlite::Row::get()`.
- Schema versioned via `PRAGMA user_version`.
- Migrations embedded in `tock-core` as `include_str!("migrations/*.sql")`.
- Each migration has a SHA-256 checksum verified at startup.
- Additive-only migrations within major versions (no destructive changes).
- Backward compatibility enforced: vaults carry a `min_compatible_version` in the header; older clients refuse to open newer vaults.

**UDA (User-Defined Attributes) storage:**
Hybrid EAV + JSON approach:
- Canonical storage: `tasks.udas TEXT (JSON)`.
- Declared UDAs projected to virtual columns via SQLite generated columns:
  ```sql
  ALTER TABLE tasks ADD COLUMN uda_energy TEXT
    GENERATED ALWAYS AS (json_extract(udas, '$.energy')) VIRTUAL;
  CREATE INDEX tasks_uda_energy_idx ON tasks(uda_energy);
  ```
- This avoids EAV JOINs on every list query (hot path) while supporting LWW-friendly diffing for sync.

**Indexes:**
Targeted indexes for common queries (Today view, urgency sort, project drill-down, time reports). Partial indexes for sparse columns (e.g., `WHERE end_ts IS NULL` for running time blocks).

**Concurrency:**
Single-writer, multiple-reader (SQLite default). Vault locked at process level; concurrent writes from the same device serialized via `tock-storage` mutex.

## Consequences

**Positive:**
- SQLite is mature, well-tested, and available on all platforms.
- SQLCipher provides transparent page-level encryption (OS-level disk theft protection).
- Rich query support (date arithmetic, JSON extraction, FTS5 for future full-text search).
- Migrations embedded in code ensure version consistency.

**Negative:**
- SQLCipher adds ~10% overhead vs. unencrypted SQLite (acceptable trade-off).
- WASM storage (IndexedDB) requires shimming SQLite semantics (manageable for Tock's query patterns).
- Virtual columns for UDAs must be declared before indexing (one-time `ALTER TABLE` per UDA).

**Neutral:**
- No ORM means manual SQL, but Tock's query surface is well-defined (all queries documented in architecture spec).
