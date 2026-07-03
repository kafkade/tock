# ADR-014: At-rest encryption — app-layer AEAD for 1.0, SQLCipher deferred

**Status:** Accepted
**Date:** 2026-07-02

> **Amends [ADR-004](ADR-004-sqlite-app-layer-encryption.md).** ADR-004's
> *Decision* section specified the CLI would store data in an `rusqlite` database
> with **bundled SQLCipher** (AES-256-CBC page-level encryption, keyed by VK).
> The build that ships for 1.0 does **not** use SQLCipher: it stores a **plain
> SQLite** file whose *sensitive payloads* are encrypted at the **application
> layer** with `tock-crypto` (AES-256-GCM AEAD). This ADR ratifies the shipped
> app-layer-AEAD approach as the **1.0 at-rest decision**, records the security
> trade-off versus SQLCipher, and moves SQLCipher integration to a tracked
> **post-1.0** enhancement. Everything else in ADR-004 (SQLite as the engine,
> the no-ORM/typed-row strategy, `PRAGMA user_version` migrations, UDA storage,
> indexes, concurrency) stands unchanged.

## Context

This ADR closes issue [#172](https://github.com/kafkade/tock/issues/172).

ADR-004 was written as a forward-looking design and assumed the CLI would open
its database through SQLCipher so the *entire* file — every page — is encrypted
transparently under the Vault Key (VK). The implementation took a different, more
incremental path, and the two have since disagreed. That disagreement is a
governance gap to close before 1.0's stability commitment.

**What actually shipped.** The storage layer
(`crates/tock-storage/src/vault.rs`) opens a **plain SQLite** database. The vault
header lives in a `vault_meta` table; the append-only event log stores each
event's **payload** encrypted with `tock-crypto` AEAD (AES-256-GCM) under a key
derived from VK (see `docs/architecture.md` §5.3). Per-device signing-key bytes
are likewise AEAD-wrapped under a VK-derived key before storage. The build marks
its format explicitly so a future migration can detect and upgrade it
(`crates/tock-core/src/vault/header.rs`):

```rust
// tock-core/src/vault/header.rs
pub const STORAGE_LAYOUT_V0: &str = "sqlite-plain-app-aead-v0";
```

The `storage_layout` field is written into the vault header and surfaced in
`tock` status output, so the on-disk format is self-describing and a later
SQLCipher-backed layout can be introduced as a distinct, detectable marker.

**Why the deviation happened / why it is acceptable for 1.0.** App-layer AEAD:

- Keeps the sensitive payloads (event contents, device keys) confidential at rest
  under the same audited RustCrypto primitives used everywhere else in tock — no
  second, differently-implemented cryptographic stack (SQLCipher's AES-256-CBC +
  HMAC page format) to vet and keep correct.
- Shares one key hierarchy and one AAD/domain-separation discipline with the
  sync envelope, so a payload is protected identically whether it is at rest in
  the local DB or in transit to an untrusted server. This is the property that
  actually matters for tock's primary threat (a hostile sync host), and it holds
  today without SQLCipher.
- Avoids pulling a bundled OpenSSL/SQLCipher build into the WASM and
  cross-platform targets during the 1.0 push, keeping the build surface and the
  WASM bundle budget (ADR-005; `core` < 2 MB compressed) under control.

**What app-layer AEAD does *not* do.** Because the database file itself is plain
SQLite, a stolen `.db` file is **not** fully opaque the way a SQLCipher file is.
The confidential *payloads* remain AEAD-ciphertext (unreadable without VK, which
is only reachable via the two-secret URK — password **and** Secret Key, per
ADR-011), but **structural and low-sensitivity metadata is exposed at rest**:
schema version, table/index structure, row counts and sizes, event timestamps and
ordering, entity kinds/UUIDs, device IDs, and the wrapped-key material in the
header (which is useless without the password + Secret Key). SQLCipher would
reduce at-rest exposure to the fixed header only. That gap is the motivation for
keeping SQLCipher on the roadmap.

## Decision

### 1. 1.0 ships app-layer AEAD (`sqlite-plain-app-aead-v0`)

For 1.0, at-rest protection is **application-layer AEAD**, not SQLCipher:

- The database is **plain SQLite**. Migrations, indexes, and the query surface are
  exactly as ADR-004 specifies.
- **Confidential data is AEAD-encrypted before it is written**: every event
  payload and the per-device signing key are sealed with AES-256-GCM under a
  VK-derived key, with domain-separated AAD (`docs/architecture.md` §5.3, §5.1).
- VK is never stored in the clear: it is wrapped under MEK, which is derived from
  the two-secret Unlock Root Key (ADR-011). A stolen database plus a stolen header
  cannot yield VK without the password **and** the 128-bit Secret Key.
- The on-disk format is self-describing via `storage_layout = "sqlite-plain-app-aead-v0"`
  so a future format can be detected and migrated.

This is the ratified, released behavior — not a temporary stopgap that 1.0
promises to remove. The forward-compatibility guarantees of
[ADR-013](ADR-013-vault-format-versioning-policy.md) apply: a future move to
SQLCipher is a structural change that must ship with an automatic, in-place,
no-data-loss migration at a major-version boundary, gated by
`min_compatible_version` for downgrade safety.

### 2. At-rest security trade-off (recorded, not hidden)

The `.db` file is **not** fully encrypted at rest. Confidential payloads are
protected (AEAD under VK); the following is **exposed** to anyone who steals the
file:

- SQLite schema, `PRAGMA user_version`, table/index layout.
- Per-event metadata: timestamps, lamport/ordering, entity kind, entity UUID,
  device ID, ciphertext sizes (mitigated by size-bucket padding, §5.3).
- Vault header fields and wrapped-key blobs (VK wrap, salts, KDF params) — all
  useless without the password **and** Secret Key.

What is **not** exposed: task/habit/time/focus contents, note text, or any
plaintext payload — those are AEAD ciphertext. What an attacker **cannot** do:
recover VK, decrypt payloads, or forge events (AAD-pinned, per-item keys).

SQLCipher would additionally hide the schema, row/table structure, and per-event
metadata, shrinking at-rest exposure to the fixed vault header. That is a real
improvement for the **local disk-theft** threat specifically, and the reason
SQLCipher remains planned.

### 3. SQLCipher moves to a tracked post-1.0 enhancement

Bundled-SQLCipher page-level encryption is **removed as a 1.0 commitment** and
tracked as a post-1.0 enhancement ([#182](https://github.com/kafkade/tock/issues/182)).
The header already reserves the transition: `STORAGE_LAYOUT_V0` distinguishes
today's format so a future
`sqlite-sqlcipher-*` layout can be introduced with an automatic migration under
ADR-013's rules. Delivering it is scoped as: bundle SQLCipher (feature-gated for
CLI/native; excluded from WASM), key the database with VK on open, add the
`storage_layout` migration, and update the threat model to promise
header-only at-rest exposure.

## Consequences

**Positive:**

- Code and governance agree: the ADR now matches
  `sqlite-plain-app-aead-v0`, closing issue #172's acceptance criteria.
- One cryptographic stack (RustCrypto AEAD + one key hierarchy + one AAD
  discipline) covers both at-rest payloads and in-transit sync — less to audit,
  no SQLCipher page-format to vet for 1.0.
- Confidential data is genuinely protected at rest under VK, which is gated by the
  two-secret URK; the residual exposure is structural metadata only.
- Keeps the 1.0 build (and the WASM bundle budget, ADR-005) free of a bundled
  SQLCipher/OpenSSL dependency.

**Negative:**

- At-rest disk theft leaks more than a SQLCipher file would: schema and per-event
  metadata are visible (though no plaintext payloads). Users who need full-file
  at-rest opacity must wait for the SQLCipher enhancement or use full-disk
  encryption (FileVault/LUKS/BitLocker) underneath.
- A second at-rest format (SQLCipher) will eventually exist, so the migration path
  reserved by `storage_layout` becomes a real, must-implement obligation under
  ADR-013 (automatic, no data loss).

**Neutral:**

- No code change is required to enact this decision — the shipped build already
  implements it; this ADR makes it the recorded policy and corrects ADR-004,
  `SECURITY.md`, and the `docs/architecture.md` §5 threat model to match.
- The security posture against tock's *primary* adversary (a hostile sync host)
  is unchanged: that path never depended on SQLCipher — event payloads carry
  their own AEAD envelope regardless of local storage format.
