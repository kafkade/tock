# ADR-019: Server data portability & retained snapshots (export, import, PITR deferral)

**Status:** Accepted
**Date:** 2026-07-27

> **Security-sensitive.** This ADR covers a *new* server surface that turns the
> relayed ciphertext into a downloadable artifact and closes an insecure direct
> object reference (IDOR) in export
> ([#201](https://github.com/kafkade/tock/issues/201), R7). It feeds the pre-1.0
> audit ([ADR-015](ADR-015-pre-1.0-security-audit-status.md),
> [#203](https://github.com/kafkade/tock/issues/203)) and unblocks cross-server
> migration ([#202](https://github.com/kafkade/tock/issues/202)). The server
> remains an encrypted blob store per
> [ADR-002](ADR-002-end-to-end-encryption.md) and
> [ADR-011](ADR-011-account-based-self-host-two-secret-auth.md): it **never
> decrypts** — every path here moves ciphertext (the non-secret vault header and
> AEAD event payloads) only.

## Context

The self-hosted sync server (`crates/tock-server`, AGPL-3.0 per
[ADR-006](ADR-006-licensing-dual-license.md)) is a per-account, event-sourced
ciphertext relay ([ADR-003](ADR-003-event-sourced-sync.md)). Before #201 it had
three gaps that were easy to conflate but are genuinely different problems:

1. **No portable export.** There was no way for an account to pull its own data
   out of a server as a self-contained artifact. The only "backup" documented in
   `docs/self-hosting.md` was a manual `tar` of the whole data volume — an
   operator action over *all* users, not a per-account portability path, and
   useless for migrating one account to another server.
2. **Export, once added, is a classic IDOR risk.** A download endpoint guarded
   only by "the vault has a session" would let account A stream account B's
   ciphertext. The safe pattern already used by `get_vault_header` is a **double
   check** — an authenticated session (`authorize_sync`) **and** proven vault
   ownership (`require_vault_access`) evaluated *before* any read, plus the SRP
   channel-binding tag.
3. **No durability if the DB is lost before download.** A download endpoint is
   *portability*, not a *backup*: it does nothing if the server's SQLite database
   is destroyed before anyone downloads. `crates/tock-server/src/billing.rs`
   modelled only tier limits (storage/device/rate) — there was no server-side
   retention of any kind. (Note: the per-tier "history retention" in
   [ADR-007](ADR-007-monetization-open-core.md) is a *billing* horizon for how
   long event history is kept, not a disaster-recovery snapshot mechanism.)

Keeping these three distinct — in code and in docs — is the core requirement:
**export ≠ import ≠ server-retained backup.**

## Decision

### 1. Three distinct capabilities

| Capability | Mechanism | Actor | Protects against DB loss? |
| --- | --- | --- | --- |
| **Export** (portability) | `GET /v1/vaults/:id/export` + offline `tock-server admin export` | Account owner / operator | No — it is a download, not a backup |
| **Import** (restore / round-trip) | `POST /v1/vaults/:id/import` | Account owner | N/A — the inverse of export |
| **Server-retained snapshots** | Scheduled `VACUUM INTO` + keep-*N* prune | Operator (automatic) | **Yes** — the point of the feature |

### 2. Export — IDOR-safe ciphertext archive

`GET /v1/vaults/:vault_id/export` returns a single JSON **archive** of one
account's vault: its non-secret header plus the entire event log, in server
insertion order, using the same opaque encoding as the sync `pull` route. The
handler runs the double check *before any read* and never decrypts:

```rust
let auth = authorize_sync(&state, &headers).await?;   // authenticated session
verify_channel_binding(&auth, &headers)?;             // SRP channel binding
// inside spawn_blocking, BEFORE reading:
db.require_vault_access(&vault_bytes, &account_id)?;   // proven ownership
```

The archive shape is shared verbatim between the HTTP route and the offline
`tock-server admin export [--all | --account <id>]` admin command (both build the
same `VaultArchive { vault_id, header, events[] }`), so #202 migration tooling can
consume either source. True chunked/streaming transfer is **deferred**: a single
JSON body is adequate for personal-scale vaults and keeps the round-trip trivial
to verify; if large vaults make it necessary, streaming can be added without
changing the archive schema.

### 3. Import — claim-semantics restore (deliberate deviation)

`POST /v1/vaults/:vault_id/import` accepts an exported archive and writes it back,
making export a real, reversible round-trip and enabling cross-server migration.
It runs the same `authorize_sync` + `verify_channel_binding` double check, and is
idempotent — duplicate event ids are ignored by the existing
`push_event` (`INSERT OR IGNORE`).

For ownership, #201 **intentionally implements import via the push-path
claim-semantics** (`ensure_vault` + `claim_vault_for_account`) rather than the
issue's literal `require_vault_access` wording. Rationale:

- Migration and disaster restore import into a **fresh, unowned** vault on a new
  server, which `require_vault_access` would reject (`NotFound` / "not yet
  associated"). `claim_vault_for_account` claims an unowned vault for the caller.
- It is still **IDOR-safe**: `claim_vault_for_account` returns `Forbidden` for a
  vault already owned by a *different* account, so account A can never import over
  account B's data. This is proven by the `import_authorization_is_idor_safe`
  test, which imports into an **already-owned** vault and asserts `403`.
- It matches the ownership pattern the write routes already use
  (`push`, `put_header`, `put_onboarding`), so import behaves like any other
  first write to a vault rather than introducing a second ownership model.

### 4. Server-retained snapshots (scheduled, keep-*N*)

A background task (`crates/tock-server/src/retention.rs`), spawned from the
`main.rs` binary **only** (never from `serve`, so integration tests never write
snapshots), periodically produces a consistent, ciphertext-only copy of the whole
database with SQLite's online `VACUUM INTO` to a timestamped file
(`tock-server-<UTC>.db`, fixed-width UTC so lexical order is chronological), then
prunes to the newest *N*. This reuses the **same `VACUUM INTO` snapshot primitive**
as the client-side backup of
[ADR-018](ADR-018-backup-restore-format-and-modes.md) — here applied server-side
to the relayed ciphertext rather than to a client's local vault. It is **on by
default**:

| Flag | Environment | Default | Meaning |
| --- | --- | --- | --- |
| `--snapshot-interval-secs` | `TOCK_SNAPSHOT_INTERVAL_SECS` | `86400` (daily) | Seconds between snapshots; `0` disables |
| `--snapshot-dir` | `TOCK_SNAPSHOT_DIR` | `<data_dir>/snapshots` | Snapshot output directory |
| `--snapshot-keep` | `TOCK_SNAPSHOT_KEEP` | `7` | Newest *N* snapshots kept; older pruned |

`tock-server admin snapshot` forces one offline. This is the capability that makes
data survive DB loss *before* a user downloads an export; the manual volume `tar`
in `docs/self-hosting.md` remains as an out-of-band complement.

### 5. Strict WAL-replay PITR — deferred

True point-in-time recovery (continuous write-ahead-log archiving + replay to an
arbitrary instant) is **explicitly deferred**. For a personal-scale ciphertext
relay this is the wrong depth/cost trade-off for 1.0:

- Clients hold the **authoritative** copy of their data and can re-push; the
  server is a convenience relay, not the system of record.
- Per-interval snapshots already bound worst-case loss to one interval, which is
  acceptable when the client can reconcile and re-push anything newer.
- Continuous WAL shipping/replay is materially heavier operationally (archiver,
  storage growth, restore tooling, corruption handling) for marginal benefit at
  this scale.

A follow-up issue tracks WAL-archiving PITR for operators who want tighter RPO.
Tiered daily/weekly/monthly retention is likewise deferred in favour of a simple
keep-newest-*N* policy.

### 6. Client-side CLI deferred to #202

Client-facing `tock account export` / `tock account import` wrappers (a user
typing one command to round-trip through a server) are **deferred to
[#202](https://github.com/kafkade/tock/issues/202)**. #201's acceptance criteria
are met at the endpoint + admin-CLI + round-trip-test level.

**Update (#202, delivered):** those wrappers now ship as `tock account export
[--out FILE]` and `tock account import FILE`, together with the cross-server
migration guide in [docs/migration.md](../migration.md); `import` targets the
vault's bound server only (no `--server` override) so it cannot make a second
server claim the same `vault_id`, and it uploads the vault's *current* local
header rather than the archived one so replaying an old archive after a password
rotation cannot strand other devices.

## Consequences

**Positive:**

- Export closes the R7 IDOR with the same audited double-auth pattern as the rest
  of the sync surface; a session for account A can never read or write account B's
  ciphertext.
- Export ⇄ import is a verified, idempotent round-trip, unblocking cross-server
  migration (#202) without the server ever decrypting.
- Scheduled snapshots give self-hosters real disaster-recovery durability,
  on by default, with a single obvious kill switch.
- Export and admin export share one archive schema, so migration tooling has one
  format to target.

**Negative:**

- Snapshots consume disk (bounded by `--snapshot-keep`); operators on tiny
  volumes may want to lower the count or disable them.
- Worst-case data loss is one snapshot interval, not zero — the price of deferring
  WAL-replay PITR.
- A single-JSON export must be held in memory on both ends; very large vaults will
  eventually motivate the deferred streaming transfer.

**Neutral:**

- Import uses claim-semantics rather than the issue's literal `require_vault_access`
  wording (see Decision §3); functionally IDOR-safe and consistent with existing
  write routes.
- Server-retained snapshots are distinct from, and complementary to, the
  client-side encrypted backup of [ADR-018](ADR-018-backup-restore-format-and-modes.md)
  and the plain-SQLite-at-rest posture of
  [ADR-014](ADR-014-at-rest-encryption-app-layer-aead.md): the server snapshot is
  ciphertext relayed between devices, not a client's local vault.
