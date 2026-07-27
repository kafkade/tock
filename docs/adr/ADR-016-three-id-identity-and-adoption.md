# ADR-016: Three-ID identity model & local→server adoption

**Status:** Accepted
**Date:** 2026-07-26

> **Amends [ADR-011](ADR-011-account-based-self-host-two-secret-auth.md)** by
> correcting its "server-assigned `account_id`" wording (§1, §2, §6), which
> conflates two different identifiers into one. Cross-references
> **[ADR-012](ADR-012-client-account-onboarding.md)** (onboarding orchestration)
> and **[ADR-013](ADR-013-vault-format-versioning-policy.md)**: this ADR
> introduces **no** vault-format change — the vault stays `v2`. It renames and
> separates identifiers that already exist and specifies the local→server
> **adoption** flow; no key material, header layout, or KDF input changes.

## Context

Tock is local-first and end-to-end encrypted: a vault can be created and used
entirely on-device, and a server is an *optional*, zero-knowledge sync backend
(ADR-011). Turning a local-only vault into a server-backed one — **adoption** —
is the single core path still missing before 1.0 (tracked in the sync/server
hardening epic #204; adoption itself is #197).

Before that code is written, one thing must be pinned down: tock's account model
uses **three distinct identifiers**, but ADR-011 describes only one
"server-assigned `account_id`". That wording conflates a client-minted crypto
identity with a server-minted principal, and is the root of the
"confusing second identity" risk. The three IDs are real and present in the
codebase today:

- **A — client crypto `account_id`.** Minted **locally** at first use
  (`let account_id = Uuid::now_v7();`,
  `crates/tock-storage/src/vault.rs:235`), written into the vault header
  (`VaultHeader.account_id`, `crates/tock-core/src/vault/header.rs:74`), and
  folded into the Unlock Root Key derivation: `derive_unlock_root_key` salts the
  Secret-Key HKDF step with `account_id ‖ kdf_version`
  (`crates/tock-crypto/src/kdf.rs:150-164`). Because the URK roots the whole key
  hierarchy, **A is bound into the vault's cryptography and is stable for the
  life of the vault.** It exists whether or not a server is ever involved.

- **V — `vault_id`.** A UUIDv7 also minted locally in `init`
  (`crates/tock-storage/src/vault.rs:242`). It is the **sync bucket key**: the
  client addresses the server by `vault_id`
  (`HttpTransport::new(server, vault_id)`,
  `crates/tock-cli/src/commands/account.rs:129`), and the server stores events
  and the wrapped header under it. Like A, it exists in a purely local vault.

- **B — server principal `account_id`.** When a client registers with a server,
  the **server** mints its *own* `Uuid::now_v7()`
  (`crates/tock-server/src/db.rs:754`) and returns it as
  `RegisterResponse.account_id` (`crates/tock-account/src/signup.rs:167-169`).
  It is the server's row key for the account (roles, quota, SRP verifier,
  sessions). The client today **discards it**
  (`let _reg: RegisterResponse = …`,
  `crates/tock-cli/src/commands/account.rs:127`).

ADR-011's "server-assigned `account_id`" collapses **A and B** into one name.
They are neither minted by the same party nor used for the same purpose: **A**
is client crypto identity (in the header, in the KDF); **B** is a server
bookkeeping principal (in the server DB). The `VaultHeader.account_id`
doc-comment currently even reads "Server-assigned account identifier this vault
belongs to" (`crates/tock-core/src/vault/header.rs:72-74`), which is the same
drift at the code level — **A is client-minted**, not server-assigned.
(Correcting that doc-comment is left to the adoption implementation, #197; this
ADR is the normative source it will follow.)

Two more facts shape the decision:

- **Adoption must not touch the crypto identity.** Adoption registers an
  *existing* local vault with a server. It must preserve **A** and **V**, let
  the server mint **B**, upload the **already-wrapped** header, and push events.
  Re-deriving keys against B, or re-wrapping the header under B, would make the
  server authoritative over the crypto identity and **break zero-knowledge**.
- **Registration is pre-auth.** The register POST happens over TLS **before**
  SRP login; SRP channel binding covers only the later header `PUT`, not
  `register` (`crates/tock-cli/src/commands/account.rs:113-133`). This has
  threat-model consequences (below).

## Decision

### 1. Name and separate the three identifiers

Tock has exactly three account/vault identifiers. They are named, owned, and
scoped as follows and MUST be treated as distinct everywhere (code, docs, UX):

| ID | Name | Minted by / where | Lifetime & scope | Purpose | Reference |
|----|------|-------------------|------------------|---------|-----------|
| **A** | Client crypto `account_id` | **Client**, at `init`, via `Uuid::now_v7()` | Stable for the life of the vault; exists with or without a server | Bound into 2SKD (salts the Secret-Key HKDF) and the vault-key-wrap AAD; part of the crypto identity | `tock-storage/src/vault.rs:235` (mint), `tock-crypto/src/kdf.rs:150-164` (`derive_unlock_root_key`), `tock-core/src/vault/header.rs:74` (`VaultHeader.account_id`) |
| **V** | `vault_id` | **Client**, at `init`, via `Uuid::now_v7()` | Stable for the life of the vault | Sync bucket key: addresses the vault's events + wrapped header on a server | `tock-storage/src/vault.rs:242` (mint), `tock-cli/src/commands/account.rs:129` (`HttpTransport::new(server, vault_id)`) |
| **B** | Server principal `account_id` | **Server**, at `register`, via `Uuid::now_v7()` | Per (server, account); one per server the vault is adopted onto | Server's principal key: roles, quota, SRP verifier, sessions | `tock-server/src/db.rs:754` (mint), `tock-account/src/signup.rs:167-169` (`RegisterResponse.account_id`) |

Invariants:

- **A and V are client-owned and server-independent.** They are minted before
  any server exists and never change on adoption. The server never assigns,
  re-issues, or validates them as crypto identity — it only stores V as an
  opaque bucket label and receives A only as an opaque field inside the header
  blob.
- **B is server-owned and adoption-scoped.** The server is authoritative over B
  only. A vault adopted onto two different servers would have two unrelated B
  values; A and V would be identical across both (see §6/§7 on the correlation
  this enables, and §4 on why a second adoption is refused by default).
- **The client MUST persist B after adoption** (today it is discarded). B is
  needed to address the account on the server, to `disconnect`, and to detect a
  re-adopt of the same vault to the same server (idempotency).

### 2. Correction to ADR-011

ADR-011 §1, §2, and §6 describe a single **"server-assigned `account_id`"** that
is embedded in the vault header and folded into 2SKD (`account_id` in the URK
derivation and the header). That description is **superseded** by this ADR:

- The `account_id` in the **vault header** and in **2SKD** is **A — the
  client-minted crypto `account_id`** (`Uuid::now_v7()` at `init`). It is
  **not** server-assigned. ADR-011 §2's KDF block and §6's header field refer to
  **A**.
- The **server-assigned** identifier is **B**, a *separate* principal that
  exists only after adoption and never enters the header or the KDF.

Everything else in ADR-011 (2SKD, URK-rooted hierarchy, SRP over the URK,
Emergency Kit as the sole recovery path, vault-header AAD coverage) stands
unchanged. This ADR only splits the overloaded name.

### 3. Binding state & the `adopt` state machine

A vault's relationship to a server is explicit binding state — **not** inferred
from a device label. (The `"local"` label written at `init` is only a *device*
label, `crates/tock-storage/src/vault.rs:242-245`; it is not a binding state.)
The state is:

- **`LocalOnly`** — no server binding. A and V exist; there is no B, no server
  URL, no credentials. This is the default after `init`.
- **`Adopting`** — a transient state during which the client registers the
  existing vault with a chosen server.
- **`ServerBacked`** — bound to exactly one server: the client has persisted the
  server URL, B, and credentials, and the wrapped header + events have been
  pushed.

```text
                 adopt --server S --email E
   ┌───────────┐  (register A/V, mint B,        ┌────────────┐
   │ LocalOnly │─────  store wrapped header, ──▶ │  Adopting  │
   └───────────┘        initial push)            └────────────┘
        ▲                                          │        │
        │  abort / rollback                success │        │ failure
        │  (nothing consumed on server;            │        │ (server tx
        │   no local binding written)              ▼        ▼  rolls back)
        │                                   ┌──────────────┐ │
        │                                   │ ServerBacked │ │
        │        disconnect                 └──────────────┘ │
        └───────  (revoke B, reset sync ────────┘   ▲        │
                   cursor, clear credentials)       └────────┘
                                                   (retry-safe:
                                                    re-adopt idempotent)
```

Transitions:

- **`adopt` (`LocalOnly → Adopting → ServerBacked`).** Derives registration
  material from the **existing header** (not a fresh `init`); **A and V are
  unchanged**. The server registers the account, mints B, and stores the
  already-wrapped header — all in one atomic operation (§5). The client then
  persists `{server URL, B, credentials}` and records `ServerBacked`.
- **`abort` / rollback (`Adopting → LocalOnly`).** If adoption fails at any
  point, the server operation is atomic (§5) so nothing is consumed
  (no username/invite spent, no partial account), and the client writes **no**
  binding. Re-running `adopt` is safe.
- **`disconnect` (`ServerBacked → LocalOnly`).** The inverse of adopt: revoke B
  on the server, reset the local sync cursor, and clear stored credentials
  (server URL, B, bearer). **A, V, and all local data are preserved** — the
  vault returns to a fully usable `LocalOnly` state. `disconnect` never rotates
  or re-wraps keys.
- **Idempotent re-adopt (second device or retry).** A second device SRP-logs-in,
  fetches the header, and continues **only if the crypto identity matches**
  (same A and V). Re-adopting the same vault to the same server is a no-op, not a
  new account.

### 4. Authoritative-server invariant

**A vault binds to exactly one server at a time.** Once a vault is
`ServerBacked`, a second `adopt` to a *different* server MUST be **refused**
unless the user passes an explicit **`--migrate`**. This prevents split-brain,
where the same `vault_id` (V) is pushed to two servers and the two histories
diverge with no reconciliation. The split-brain exposure is concrete in the
code today: the client persists a **single** global server URL
(`crates/tock-storage/src/sync/mod.rs:38-66`) and a **single** credential entry
(`crates/tock-cli/src/commands/account.rs:296-319`), and the `"local"` label is
only a device label, not a binding guard
(`crates/tock-storage/src/vault.rs:242-245`) — so nothing currently stops the
same vault from being adopted onto a second server.

- The single persisted `{server URL, B, credentials}` entry is the source of
  truth for the binding. The client enforces the invariant before starting
  `adopt`.
- `--migrate` is the deliberate, explicit path to move a vault to a new server;
  it `disconnect`s the old binding (or marks it superseded) and adopts the new
  one. Its full reconciliation semantics are specified with the implementation
  (#199).

### 5. Register-and-claim must be atomic, validated, normalized, retry-safe

The server-side of adoption — create the account (mint B), claim the vault
bucket (V), and store the wrapped header — MUST execute as **one atomic unit**
so a partial failure cannot consume a username/invite or leave a vault
half-bound. Specifically (detailed acceptance in #199):

- **Atomic:** register + vault-claim + header-store run in a single DB
  transaction; on any failure nothing is consumed and the operation is
  retry-safe.
- **Validated:** the server parses the submitted header and cross-checks
  `vault_id` (V) against the header before storing; it does not accept
  inconsistent public material.
- **Normalized:** username/identifier uniqueness is enforced with consistent
  case + Unicode normalization at both register and login (uniqueness already
  exists; the work is normalization, not re-adding it).
- **Retry-safe / idempotent:** re-running a failed or partially applied adopt
  converges to the same state without creating duplicate principals.

### 6. Zero-knowledge invariant & honest metadata disclosure

**Zero-knowledge invariant (precise):** No plaintext and no key material ever
leaves the device. The server receives only: the SRP verifier and salt, the
public KDF parameters, `vault_id` (V), and the **already-wrapped** vault header
(the VK is wrapped under MEK ← URK). It never receives the password, the Secret
Key, the URK, the MEK, the VK, any item key, or any decrypted content. Adoption
does **not** relax this: it uploads the *existing* wrapped header and encrypted
events unchanged.

Zero-knowledge is **not** zero-metadata. Being honest about what a server (or a
network observer of registration, which is pre-auth/TLS-only) *can* see:

| Item disclosed to the server | What it reveals | Notes |
|------------------------------|-----------------|-------|
| **A** (inside the header blob) | An opaque 16-byte value | UUIDv7 ⇒ **embeds the vault's creation timestamp**; identical across every server this vault is adopted to (correlation, §7) |
| **V** (`vault_id`, bucket key) | An opaque bucket label | UUIDv7 ⇒ embeds creation time; identical across servers (correlation, §7) |
| **B** (server principal) | Account row key on this server | Server-local; UUIDv7 ⇒ embeds account-creation time |
| **KDF parameters** | Argon2id cost, `kdf_version`, salts | Public by design; needed for new-device unlock |
| **SRP verifier + salt** | Enables login; offline-crack-resistant | Uncrackable without the 128-bit Secret Key (ADR-011) |
| **Email** | The account's contact identifier | Bound at adopt (§9); an identifying handle |
| **Account graph** | Which vault (V) belongs to which principal (B), device count, event volume/timing, blob sizes (bucketed) | Classic sync metadata; padding buckets blob sizes but not event cadence |

The one change that would truly break zero-knowledge — and is therefore
**forbidden** — is asking the server to assign the crypto identity (A) or
re-wrapping the header under B. The server owns **B only**; A and V stay
client-owned.

### 7. UUIDv7 correlation and its mitigation

A and V are UUIDv7 values. UUIDv7 **embeds a millisecond creation timestamp**,
and because A and V are minted once at `init` and reused verbatim on every
adoption, **the same A/V appear on every server a vault is adopted to** — a
cross-server correlation handle (and a creation-time leak). To reduce this:

- **Externally, prefer opaque per-server aliases.** What a server stores/keys on
  SHOULD be an **opaque per-server alias** of V (and, where a header-external
  handle is needed, of A) rather than the raw UUIDv7 — so two servers cannot
  trivially correlate the same vault, and no creation timestamp is exposed in
  the wire identifier. The internal, header-bound A (which the KDF depends on)
  does not change; only the *externally disclosed* identifier is aliased.
- **Alternatively, mint V (and header-external identifiers) as UUIDv4** to drop
  the embedded timestamp. Changing A's on-disk form is out of scope here (it
  would touch the KDF/header and thus the format); the low-cost win is the
  external alias / v4 for the identifiers the server sees.

The concrete alias scheme is specified with the server work (#199). This ADR
records the **requirement**: the identifiers a server sees SHOULD NOT be
raw, reused, timestamp-bearing UUIDv7 values.

### 8. Threat model (summary)

This is a security-sensitive path. Key threats and the stance taken:

- **Server compromise (data at rest).** The server holds ciphertext, the wrapped
  header, an SRP verifier, and metadata (§6). It cannot derive keys or read
  plaintext; a stolen verifier is uncrackable without the 128-bit Secret Key
  (ADR-011). **Mitigated by design.**
- **Malicious/curious server tries to become authoritative over identity.**
  Defended by §2/§6: A and V are client-owned; the server never assigns them and
  the header is never re-wrapped under B. A server that returns a different A, or
  asks the client to re-wrap, is out of spec and MUST be rejected by the client.
- **Registration is pre-auth (TLS-only).** `register` runs before SRP login, so
  SRP **channel binding does not cover it** — only the later header `PUT` is
  channel-bound. An active network attacker who breaks/strips TLS at register
  time could tamper with the *public* registration material (email, SRP
  verifier, KDF params, V). This cannot expose secrets (none are sent), but it
  motivates the server-side validation in §5 (parse + cross-check header vs V)
  and standard TLS hardening; a future channel-bound register is a candidate
  follow-up.
- **Split-brain across two servers.** Defended by the authoritative-server
  invariant (§4): a second adopt is refused without `--migrate`.
- **Cross-server correlation / creation-time leak.** Reduced by the opaque
  per-server alias / v4 requirement (§7).
- **Partial adoption / consumed username on failure.** Defended by the atomic,
  retry-safe register-and-claim (§5).

### 9. Bind email at adopt, not at init

The URK derivation does **not** depend on email: `vault::open` derives keys from
password + Secret Key + header only
(`crates/tock-storage/src/vault.rs:203`, the `derive_unlock_root_key` call).
Therefore a local-only vault needs **no email**, and requiring one at `init`
would add a server-shaped, identifying field to a purely on-device flow.

**Recommendation:** the first-run/local onboarding (#198) SHOULD keep the vault
email-free (prompt for a local *name* instead), and email SHOULD be bound at
**`adopt`** time, when the user is deliberately choosing a server and email is
actually needed (account contact, sign-in). This keeps `LocalOnly` genuinely
local and minimizes metadata for users who never adopt a server. (See Open
Question Q1.)

## Open questions

- **Q1 — email binding timing.** This ADR recommends binding email at `adopt`,
  not `init` (§9). The first-run gate (#198) tracks the exact UX (email-free
  local name vs. optional email); the recommendation here is "email-free until
  adopt". Resolve alongside #198.
- **Q2 — alias scheme for V/A on the wire.** §7 requires an opaque per-server
  alias (or v4) for disclosed identifiers; the concrete scheme is specified with
  the server work (#199).
- **Q3 — `--migrate` reconciliation semantics.** §4 mandates refusal of a second
  adopt without `--migrate`; the precise migrate/reconcile behavior is specified
  with #199.

## Consequences

**Positive:**

- **One unambiguous vocabulary (A / V / B)** across code, docs, and UX, removing
  the "confusing second identity" risk before any adopt code is written.
- **Zero-knowledge is preserved by construction:** adoption keeps A and V
  client-owned and uploads the existing wrapped header; the server owns only B.
- **A safe, explicit adoption lifecycle** (`LocalOnly → Adopting → ServerBacked`
  with `abort` and `disconnect`) and an authoritative-server invariant that
  prevents split-brain.
- **Honest metadata posture:** the disclosure table and UUIDv7 correlation
  mitigation make the privacy trade-offs explicit rather than implied.

**Negative:**

- **The client must now persist and manage B** (previously discarded) and track
  explicit binding state, adding fields to credential storage and status output.
- **Correcting the A/B conflation touches wording in several places** (this ADR,
  ADR-011 references, the header doc-comment fixed downstream), a one-time
  documentation cost.
- **An opaque per-server alias (§7)** adds a small mapping layer on the server
  relative to keying directly on the raw `vault_id`.

**Neutral:**

- **No vault-format change:** the vault stays `v2` (ADR-013); A, V, the header
  layout, and the KDF are unchanged. This ADR renames/separates and specifies a
  flow; it does not alter any bytes on disk.
- ADR-011's crypto (2SKD, URK-rooted hierarchy, SRP over URK, Emergency Kit) is
  untouched — only the overloaded `account_id` name is split.
- No CI job names change; adoption/server work lands under existing gates
  (mirror any *new* merge-gate jobs in `kafkade/github-infra` `repo_tock.tf`,
  per repo policy).

## Implementation impact (downstream, not in this ADR)

- **#197 — `tock account adopt`.** Implements the `LocalOnly → ServerBacked`
  transition from the existing header (A/V preserved, B minted, wrapped header
  uploaded, initial push), persists B + binding state, enforces §4, and defines
  `tock account disconnect`. Also fixes the `VaultHeader.account_id`
  doc-comment drift noted in Context.
- **#198 — first-run local onboarding gate.** Forces a real password + saved
  Emergency Kit and keeps the local vault email-free per §9/Q1.
- **#199 — atomic register+claim, normalization, split-brain guard.** Implements
  §5 (atomic, validated, normalized, retry-safe) and the §4 client-side
  authoritative-server enforcement, plus the §7 alias scheme (Q2) and the
  `--migrate` reconciliation semantics (Q3).
