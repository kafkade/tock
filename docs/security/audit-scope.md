# tock — External Security & Cryptography Audit Scope

**Status:** Pre-engagement scoping brief (living document)
**Owner:** tock maintainers
**Related:** [ADR-015](../adr/ADR-015-pre-1.0-security-audit-status.md) ·
[SECURITY.md](../../SECURITY.md) · [Issue #173](https://github.com/kafkade/tock/issues/173)

This document scopes an independent external review of tock's cryptographic
protocol and its implementation. It exists so that "commission an audit" is a
hand-off, not an open research task: the surfaces below, the pointers, and the
findings process are ready for a prospective auditor (a firm or a qualified
community reviewer) to price and execute.

As of 1.0, tock is **unaudited (pre-audit)** — see
[ADR-015](../adr/ADR-015-pre-1.0-security-audit-status.md). Only the underlying
RustCrypto primitives have had independent review; tock's own composition of them
has not.

## Objectives

Assess whether tock delivers the guarantees it markets:

1. **End-to-end confidentiality** — the sync server and any network observer see
   only opaque ciphertext plus low-sensitivity metadata; never plaintext.
2. **Zero-knowledge server** — the server can store, route, and serve encrypted
   blobs and authenticate users **without** the ability to decrypt user data or
   recover secrets from what it holds.
3. **Sound key hierarchy & recovery model** — keys are derived, separated,
   wrapped, and zeroized correctly; the two-secret model behaves as claimed; the
   documented "lost Secret Key = unrecoverable" property actually holds.
4. **Authentication integrity** — SRP-6a and the derived session/channel binding
   resist impersonation, downgrade, and replay.

## In scope

### 1. Key hierarchy & two-secret key derivation (2SKD)

- Derivation chain: `URK = Argon2id(password, kdf_salt) XOR HKDF(secret_key, account_id, ...)`
  → MEK → wraps VK → per-domain / per-item keys.
- Argon2id parameters and downgrade resistance; salt independence
  (`kdf_salt` vs `salt_srp`); Secret Key entropy (128-bit) and encoding.
- HKDF domain separation across contexts; absence of cross-context key reuse.
- Key wrapping (AES-256-GCM envelope) at each tier; VK never stored/derived in
  the clear.
- Memory hygiene: `Zeroize`/`ZeroizeOnDrop` coverage, redacted `Debug`, absence
  of key material in logs/errors/panics.
- Recovery model: Emergency Kit (Secret Key + sign-in address) as the sole
  recovery path; unrecoverability guarantee when Secret Key is lost.
- References: [ADR-002](../adr/ADR-002-end-to-end-encryption.md),
  [ADR-011](../adr/ADR-011-account-based-self-host-two-secret-auth.md),
  [ADR-012](../adr/ADR-012-client-account-onboarding.md).

### 2. Vault format & AEAD usage

- Per-item envelope encryption; per-item IK derivation; nonce generation
  (uniqueness, CSPRNG source; `getrandom` on wasm32).
- **AAD / domain separation:** correctness and completeness of the AAD binding
  `"tock|v1|" || entity_kind || "|" || entity_id || "|" || op || "|" || lamport || "|" || device_id`;
  domain tags (`tock-vault-wrap-v1`, `tock-item-wrap-v1`, `tock-recovery-wrap-v1`).
- **Size-bucket padding** — does bucketing (512B/2KB/8KB/32KB and the
  power-of-two scheme in ADR-002) actually bound metadata leakage as claimed.
- At-rest model: app-layer AEAD over a plain SQLite file; verify the documented
  residual metadata exposure matches reality and no plaintext payload leaks.
- Vault header / `storage_layout` marker and forward-compatibility policy.
- References: [ADR-002](../adr/ADR-002-end-to-end-encryption.md),
  [ADR-013](../adr/ADR-013-vault-format-versioning-policy.md),
  [ADR-014](../adr/ADR-014-at-rest-encryption-app-layer-aead.md).

### 3. SRP-6a handshake, session/token derivation & channel binding

- SRP-6a (RFC 5054) usage: group parameters (4096-bit safe prime, SHA-256),
  verifier derivation from URK (not password directly), and handshake message
  flow.
- Session key → token/channel-binding derivation (#130): binding the
  authenticated session to the transport so a TLS-stripping MITM cannot
  impersonate.
- Downgrade, replay, and verifier-leakage resistance; server-side storage of the
  verifier and offline-cracking cost given the Secret Key requirement.
- References: [ADR-010](../adr/ADR-010-srp-authentication.md),
  [ADR-011](../adr/ADR-011-account-based-self-host-two-secret-auth.md),
  `docs/architecture.md` §5.6.

### 4. Sync protocol & conflict path

- Event-sourced wire format; per-event AEAD envelope; ordering via
  Lamport clock + UUIDv7; duplicate/replay rejection; reorder handling.
- Conflict detection and the user-review resolution path (no silent
  last-write-wins for user data); ensure conflict handling cannot induce
  plaintext exposure or key confusion.
- Device registry / pairing and per-device signing keys.
- References: [ADR-003](../adr/ADR-003-event-sourced-sync.md),
  `docs/architecture.md` §5.

### 5. Server zero-knowledge claims

- Confirm the server (`crates/tock-server`, Axum) never has access to plaintext
  or key material: blob storage, vault-header bootstrap
  (`PUT`/`GET /v1/vaults/:id/header`), `srp/start` returning `kdf_params`.
- Authentication and authorization boundaries; multi-user self-host
  (one `admin` + N `user`) isolation.
- What metadata the server necessarily learns (event count, timing, bucketed
  sizes, device IDs) vs. what it must never learn.

## Out of scope

- Third-party forks or unofficial builds.
- Upstream dependency internals (RustCrypto primitives) — already independently
  reviewed; report issues to those maintainers.
- Endpoint compromise / malicious OS while the vault is unlocked (explicitly not
  protected — see threat model).
- Social engineering / phishing for the master password; physical access to an
  unlocked device.
- General web-app or CLI functional-bug hunting unrelated to the cryptographic
  guarantees (unless it breaks a security boundary above).
- Availability / DoS of the sync server (operational, not a confidentiality
  guarantee).

## Artifacts & pointers for auditors

| Area | Where |
|------|-------|
| Crypto primitives & key types | `crates/tock-crypto/` (`aead.rs`, `kdf.rs`, `keyexchange.rs`, `signature.rs`, `secret.rs`, `random.rs`) |
| Account material, SRP state machine, 2SKD | `crates/tock-account/` |
| Vault lifecycle, event log, wrapping | `crates/tock-storage/` (`vault.rs`, `event_log.rs`), `crates/tock-core/src/vault/` |
| Sync protocol foundation | `crates/tock-sync/` |
| Server (zero-knowledge blob store) | `crates/tock-server/` |
| Design rationale | `docs/adr/ADR-002, -003, -010, -011, -012, -013, -014, -015` |
| Threat model | `docs/architecture.md` §5.5 |

## Deliverables expected from the review

- A findings report with severity ratings and reproduction detail.
- Concrete remediation guidance per finding.
- A public summary/attestation suitable for linking from `SECURITY.md`.

## Findings-handling process

1. Findings are received via GitHub private vulnerability reporting (see
   [SECURITY.md](../../SECURITY.md)) or a direct channel agreed at engagement.
2. Each finding is triaged, assigned a severity, and tracked to remediation
   before public disclosure, coordinating timing with the reviewer.
3. On completion, we publish a summary/attestation and **update** the audit-status
   disclosure in `SECURITY.md`, `README.md`, and `docs/architecture.md` §5.5 —
   the "unaudited (pre-audit)" notice is only lifted once a genuine review has
   landed and its findings are addressed.
