# ADR-018: Encrypted backup & restore — format and restore modes

**Status:** Accepted
**Date:** 2026-07-26

> **Security-sensitive.** Backup is a *new* surface that turns the vault into a
> portable artifact, so it needs a written format and threat model *before*
> implementation ([#200](https://github.com/kafkade/tock/issues/200), W4). This
> ADR builds directly on [ADR-002](ADR-002-end-to-end-encryption.md) (per-item
> envelope encryption) and [ADR-014](ADR-014-at-rest-encryption-app-layer-aead.md)
> (app-layer AEAD, plain-SQLite at rest), and inherits the key hierarchy and
> two-secret unlock of [ADR-011](ADR-011-account-based-self-host-two-secret-auth.md)
> and the format-stability guarantees of
> [ADR-013](ADR-013-vault-format-versioning-policy.md). It closes issue
> [#196](https://github.com/kafkade/tock/issues/196).

## Context

tock is local-first. For a user who never enables sync, the local vault is the
**only copy** of their data; for a user who does sync, the server is an
encrypted blob store that holds ciphertext only and cannot reconstruct anything
without the client's keys. Either way, a first-class **backup & restore** path is
required — and because it produces a file that leaves the originating device, it
must be designed as a security surface, not bolted on as a file copy.

### The naïve design leaks plaintext

Per [ADR-014](ADR-014-at-rest-encryption-app-layer-aead.md), the 1.0 build stores
a **plain SQLite** database whose *event-log payloads* and *device signing keys*
are app-layer AEAD (AES-256-GCM under VK-derived keys), but whose **materialized
domain tables are plaintext at rest**. This is not a corner case — it is where the
readable data lives:

- `tasks.title`, `tasks.notes` (`crates/tock-storage/src/migrations/0003_domain_tables.sql`)
- every habit text field — `title`, `identity`, `cue`, `craving`, `response`,
  `reward`, log `notes` (`0006_habits.sql`)
- `checklist_items.title` (`0014_checklist_items.sql`)

Consequently, **copying the SQLite file or exporting materialized state produces a
plaintext archive.** The convenient "backup needs no new key, the data is already
encrypted" assumption is only true for an **event-log-only** archive (whose payloads
are AEAD ciphertext), *not* for a full snapshot of the live database.

### Two failure modes that a careless format would hit

Both are grounded in the current implementation:

1. **Un-synced edits are not yet events.** Domain repositories write directly to
   SQLite and never emit events inline; signed, AEAD-encrypted events are
   **synthesized at sync time** by diffing domain state
   (`crates/tock-storage/src/sync/mod.rs::collect_local_changes`). An
   event-log-only archive captured before a sync therefore **silently omits every
   local change that has not yet been turned into an event.**

2. **Device identity and the Lamport clock live inside the vault.** Each event is
   stamped with the local `device_id` and a per-device monotonic `lamport`, and the
   next value is allocated as "max-for-this-device + 1"
   (`crates/tock-storage/src/event_log.rs` — `append` / `next_local_lamport`). If a
   backup is restored onto a *second, still-live* device without changing identity,
   two writers share one `device_id` and one Lamport sequence, which breaks the
   ordering and duplicate-rejection invariants sync relies on. Worse, sync **pushes
   local events before it reconciles remote history**
   (`crates/tock-cli/src/commands/sync_cmd.rs` — push precedes pull/ingest), so a
   stale restore can shove old state ahead of newer remote state.

A backup ADR must answer all three: keep the archive confidential, make it
**complete** (capture un-synced state), and make restore **safe for sync** (never
create a second writer with the original identity).

## Decision

### 1. Default format: outer-encrypted full snapshot

The default and recommended backup is a **single outer-encrypted file** wrapping a
**consistent snapshot of the whole SQLite database** (produced with SQLite's online
backup / `VACUUM INTO`, so it is transactionally consistent and includes the
materialized domain tables, the event log, sync bookkeeping, and app state).

The snapshot is encrypted under a **domain-separated backup key**, never under VK
directly and never reusing a per-item key:

```text
salt   ← 32 random bytes (fresh per backup)
BK     ← HKDF-SHA256(ikm = VK, salt = salt, info = "Tock/v1/backup")
nonce  ← 12 random bytes (fresh per backup)
body   ← AES-256-GCM(key = BK, nonce = nonce, aad = manifest, plaintext = snapshot)
```

The random per-backup `salt` (a deliberate departure from the empty-salt
`tock/v1/*` derivations used for domain/item keys) makes `BK` unique to each backup
even for the same VK, and the `"Tock/v1/backup"` label domain-separates it from the
sync snapshot key (`"Tock/v1/snapshot/…"`, architecture §6.4) and every other
derivation.

**Authenticated manifest.** A cleartext-but-authenticated manifest is bound into the
AEAD as **AAD** (so any edit invalidates the tag) and carries at least:

| Field | Purpose |
| --- | --- |
| `format_tag` + `format_version` | Identify the archive; gate future format changes. |
| `account_id` (A) | Bind the backup to its account (mirrors the vault-header/AAD binding). |
| `vault_id` (V) | Bind the backup to its vault; refuse cross-vault restore. |
| `kdf_version` | Record the KDF generation so restore can re-wrap if it has advanced. |
| `event_high_water_mark` | Per-device highest `lamport` (and global count) at snapshot time. |
| `snapshot_hash` | SHA-256 of the plaintext snapshot, for integrity + rollback detection. |
| `salt`, `nonce`, `created_at` | Reproduce `BK` / open the body; record provenance. |

Binding the manifest as AAD is what **defeats truncation and rollback**: per-item
event AEAD only authenticates each event *in isolation*, so it cannot detect a
short-copied file, a dropped tail of events, or an older archive swapped in. The
`event_high_water_mark` + `snapshot_hash`, authenticated as a unit, let restore
reject an archive that is incomplete or stale relative to what it should contain.

The on-disk layout is self-describing:

```text
[ magic ][ format_version ][ authenticated manifest ][ AES-256-GCM ciphertext + tag ]
```

### 2. Alternative format: event-log-only archive (documented, not default)

An **event-log-only** archive exports the append-only event log (whose payloads are
already per-item AEAD ciphertext per ADR-002/ADR-014) plus the device registry and
clock metadata. It is smaller and needs no new key *for the payloads*, but it is
**not** the default because:

- It is only complete if it **first synthesizes the un-synced events** that
  `collect_local_changes` would produce at sync time; otherwise it drops local edits
  that never became events (see Context). Restore then depends on replaying events to
  rebuild the materialized tables.
- It **still requires an authenticated manifest** (format tag, `account_id`,
  `vault_id`, high-water mark, hash) to defeat truncation/rollback — per-item AEAD
  authenticates each event, not the archive's *completeness*.

The full snapshot is the default because it captures the entire materialized state
(including sync bookkeeping and app state) in one consistent image, restores without
an event-replay step, and does not hinge on the sync-synthesis path having run.

### 3. Two restore modes

Restore behavior depends on whether the original device's **identity** is being
resumed or a new device is being created. The mode is an explicit choice, not
inferred.

**A. Disaster recovery — the original device is gone.**
Restore in place and **keep the original `device_id`, signing key, and Lamport
clock**. Reuse the stored sync cursor / server binding and resume sync normally.
Because the original device is truly gone, there is exactly one writer for that
identity, so ordering and duplicate-rejection invariants hold. Precondition: the
original device is not still live and syncing — otherwise this becomes a clone.

**B. Clone / second-device restore — the original may still exist.**
Before the restored vault is allowed to sync it must:

1. **Mint a new `device_id` and a fresh Ed25519 signing key**, and register that
   new device (so it gets its own Lamport sequence — no shared identity).
2. **Reset the sync cursor and server binding** so it does not resume from the
   original device's position.
3. **Reconcile remote history first** — pull and ingest the server's events **before
   any push**. This is mandatory because the client pushes before it reconciles
   (`sync_cmd.rs`); reconciling first prevents a stale restore from pushing old
   state ahead of newer remote state.

Skipping any of these reproduces the "two writers, one identity" and
"stale-push-ahead" hazards from the Context.

### 4. Restore needs only password + Secret Key — no new user secret

Both modes decrypt the same way the vault always does: **password + Secret Key**
derive the URK → MEK, which unwraps VK, which re-derives `BK` from the manifest
`salt`. The backup introduces a new **cryptographic** key (`BK`), **not** a new
**user-managed** secret — there is nothing extra for the user to memorize or store
beyond what they already hold.

It follows that the **Emergency Kit / Secret Key must be stored separately from the
backup file** (mirroring `docs/self-hosting.md` §8). A backup file and the Secret
Key kept together are a single point of compromise; kept apart, a stolen backup is
inert. As with all of tock, losing the Secret Key with no Emergency Kit is
unrecoverable by design (ADR-011) — a backup does **not** change that.

### 5. Backup is not export

`tock-export` produces **plaintext, lossy** output for **portability** (moving data
to another tool). It is explicitly **not** a backup:

| | Backup (this ADR) | Export (`tock-export`) |
| --- | --- | --- |
| Confidentiality | Encrypted (outer AEAD under `BK`) | **Plaintext** |
| Fidelity | Lossless, restorable | Lossy (no keys, clocks, sync state) |
| Purpose | Recover *this* vault | Move data elsewhere |

Any plaintext escape hatch (e.g. a `--plaintext-json` flag on the export path) must
carry a **loud, explicit warning** that the output is unencrypted and is not a
backup, so a user cannot mistake a portability dump for a protected archive.

### 6. W4 (#200) ships behind this ADR

Implementing backup/restore (#200) **must not** proceed as "no ADR needed." The
plaintext-at-rest reality above means an unreviewed backup feature would ship a
plaintext archive by default. This ADR is the prerequisite artifact; #200
implements the format and modes defined here.

## Consequences

**Positive:**

- A stolen backup file is opaque: the body is AES-256-GCM under `BK`, reachable only
  via VK, which is gated by the two-secret URK (password **and** Secret Key).
- The authenticated manifest (format tag, account/vault binding, high-water mark,
  snapshot hash) detects truncation, rollback, and cross-vault/misdirected restore —
  gaps that per-item event AEAD alone cannot cover.
- Restore is safe for sync: disaster recovery keeps one writer per identity, and
  clone restore mints a new identity and reconciles before pushing, so it cannot
  corrupt the Lamport ordering or push stale state ahead of remote.
- No new user-managed secret: restore reuses the existing password + Secret Key.
- The backup-vs-export boundary is explicit, so users don't mistake a plaintext
  portability dump for a protected backup.

**Negative:**

- The full-snapshot default is larger than an event-log-only archive and re-encrypts
  the whole database on each backup (no incremental format in 1.0).
- Correct clone restore is a multi-step, mandatory sequence (new identity → reset
  cursor → reconcile-before-push); getting the ordering wrong is a real footgun that
  the implementation and UX must guard.
- A new keyed format (`BK`, manifest, versioned envelope) is one more thing to keep
  correct and to migrate under ADR-013's format-stability rules.

**Neutral:**

- This ADR defines the format and modes; the implementation lands in
  [#200](https://github.com/kafkade/tock/issues/200).
- The backup snapshot is distinct from the **sync** snapshot/compaction (architecture
  §6.4): the sync snapshot is a per-vault event-store cache keyed by
  `HKDF(VK, "Tock/v1/snapshot/…")`; a backup is a self-contained, outer-encrypted
  archive for recovery. They share the VK root but not the key label or purpose.
- If SQLCipher lands post-1.0 (ADR-014), the snapshot source becomes an
  already-encrypted file; the outer `BK` envelope and manifest still apply and remain
  the authenticated, portable unit.
