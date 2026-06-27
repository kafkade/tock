# ADR-011: Account-based self-host with two-secret (1Password-style) auth

**Status:** Accepted (2SKD crypto/core/storage landed in #126; SRP verifier,
server-assigned accounts, and Emergency-Kit onboarding UX tracked separately)
**Date:** 2026-06-26

> **Amends [ADR-002](ADR-002-end-to-end-encryption.md)** (replaces the password-only
> Master Key derivation and removes the 24-word recovery key) and
> **[ADR-010](ADR-010-srp-authentication.md)** (the SRP verifier now derives from the
> two-secret unlock key, not the password directly). See those ADRs for the superseded
> details.

## Context

Tock is end-to-end encrypted and local-first, but three facts about the current design
limit how it can be deployed and recovered:

1. **Crypto is password-only.** `tock_core::vault::KeyHierarchy::derive_master_key(password,
   &header)` derives the Master Key (MK) from the password plus the per-vault Argon2id salt
   (`tock-crypto/src/kdf.rs`). There is **no second secret**. The strength of the entire vault
   is bounded by the entropy of a human-chosen password, and a stolen server SRP verifier (once
   SRP ships) is offline-crackable at password strength.
2. **SRP is unimplemented.** [ADR-010](ADR-010-srp-authentication.md) specifies SRP-6a, but no
   SRP code exists — `tock-crypto/src/keyexchange.rs` is X25519/Ed25519 Diffie–Hellman, not SRP.
   This is the moment to fix the *input* to SRP before any verifier format is locked in.
3. **Accounts are hosted-mode-only.** `tock-server/src/accounts.rs` returns `404` unless the
   server runs in `--mode hosted`; the schema (`db.rs`) is `accounts(id, email, api_token, tier)`
   with random-bearer (`tok_…`) auth. **Self-hosted mode has no accounts and no auth** — vaults,
   devices, and events are anonymous, keyed only by `vault_id`. Multi-device today is
   device-to-device **pairing** (QR + 6-word phrase, [ADR-003](ADR-003-event-sourced-sync.md)),
   not account sign-in.

We want the deployment and account experience of **Immich** (one self-hosted server, an admin,
and many user accounts who sign in from any device) combined with the cryptographic posture of
**1Password** (a password the user knows *plus* a high-entropy Secret Key the user has, so a
stolen server database is uncrackable). Crucially, "Immich-like" applies to **deployment and
account UX only** — it must **not** make the server authoritative over plaintext. The system
stays zero-knowledge, E2EE, and local-first: the server only ever stores ciphertext and an SRP
verifier.

Two reconciliations are required and have been **confirmed**:

- **Recovery model.** We adopt the pure 1Password model: the **Emergency Kit is the only
  recovery path**. The 24-word recovery key / `vk_recover_ct` from
  [ADR-002](ADR-002-end-to-end-encryption.md) is **dropped**. Losing the Secret Key with no
  Emergency Kit is **unrecoverable by design**.
- **Multi-tenancy.** Self-hosted Tock is **multi-user**: one `admin` plus N `user` accounts,
  Immich-style.

## Decision

Tock adopts a **two-secret key derivation (2SKD)** rooted in an account **password** (something
the user knows) and a client-generated **Secret Key** (something the user has). The combined
output is the **Unlock Root Key (URK)**, which both (a) seeds the SRP authentication secret and
(b) roots the existing vault key hierarchy. The server never receives the password or the Secret
Key.

### 1. Secret Key

- **Entropy:** 128 bits from a CSPRNG (1Password parity). Generated **client-side** at account
  creation and **never transmitted to the server**.
- **Encoding (Emergency Kit form):** a version tag, the account ID, and the Crockford Base32
  encoding of the 128-bit secret, grouped for transcription, with a checksum group:

  ```text
  A4-<ACCOUNTID>-<G1>-<G2>-<G3>-<G4>-<G5>-<G6>-<CK>
  │   │           └──────── 128-bit secret, Crockford Base32, 6 groups ───────┘  │
  │   └ account id (Crockford Base32 of the account UUID, server-assigned)        │
  └ format/version tag ("A4" = account-key format v4-style; bump on KDF change)   └ 4-char checksum
  ```

  Crockford Base32 (no ambiguous `I L O U`, case-insensitive on input) is reused from the
  former recovery-key encoding. The checksum group is a truncated CRC over the version tag,
  account ID, and secret, so a mistyped Secret Key is rejected before any Argon2id work.

- **At rest on the client:** the Secret Key is stored in the platform keystore (Keychain /
  Secret Service / DPAPI) on devices that have completed sign-in, so the user is not re-prompted
  every unlock. It is **never** written to the vault file or sent to the server.

### 2. Two-secret key derivation (2SKD)

The URK is the XOR of two independent 32-byte streams — one rooted in the password, one in the
Secret Key — à la 1Password's 2SKD:

```text
salt           = vault.kdf_salt              (16 B, random per account/vault)
account_id     = vault.account_id            (16 B, server-assigned account UUID)
kdf_version    = vault.kdf_version            (u16; selects params + info strings)

K_pw  = Argon2id(password, salt, params=TOCK_V1)                        // 32 B  (something you know)
K_sk  = HKDF-SHA256(ikm = secret_key,
                    salt = account_id,
                    info = "Tock/2skd/v1/secret-key", len = 32)         // 32 B  (something you have)

URK   = K_pw XOR K_sk                                                   // 32 B  Unlock Root Key
```

Properties:

- **Stolen server data is uncrackable.** The server holds at most the SRP verifier (§4) and
  ciphertext. Recovering the URK by brute force requires guessing the password **and** the
  128-bit Secret Key; the latter alone makes offline attack infeasible (≈2¹²⁸ work), independent
  of password strength.
- **XOR-combine is information-theoretically clean.** Neither input is recoverable from URK
  without the other; both streams are full-width (32 B) and independently derived.
- **Versioned.** `kdf_version` selects the Argon2id parameters and the HKDF `info` labels.
  Bumping it (e.g. for stronger Argon2id memory cost) is a forward-compatible migration:
  re-derive URK, re-wrap MEK→VK, push a new SRP verifier.

### 3. Rooting the vault hierarchy at the URK

The URK **replaces the Master Key (MK)** as the top of the hierarchy from
[ADR-002](ADR-002-end-to-end-encryption.md). Everything below MK is unchanged:

```text
URK ──HKDF(info="Tock/v1/mek")──► MEK ──AES-256-GCM key-wrap──► VK ──► DK_kind ──► IK
```

- `MEK = HKDF-SHA256(ikm = URK, salt = vault.hkdf_salt, info = "Tock/v1/mek", len = 32)`.
- `MEK` wraps the random per-vault `VK` exactly as today (`vk_wrap_ct`, `vk_wrap_nonce`).
- `VK → DK_kind → IK`, per-item AEAD, size-bucket padding, and AAD discipline are **unchanged**
  (architecture §5.3).
- **Password change** re-derives URK from the *same* Secret Key + new password, re-derives MEK,
  and re-wraps VK. The Secret Key does not change.
- **VK rotation** is unchanged (re-wrap, or full re-encryption).

### 4. SRP verifier over the 2SKD output

SRP-6a ([ADR-010](ADR-010-srp-authentication.md)) is retained, but its private exponent `x`
derives from the **URK**, not from the password directly:

```text
x = HKDF-SHA256(ikm = URK, salt = salt_srp, info = "Tock/v1/srp-x", len = 32)   // mod N as per RFC 5054
v = g^x mod N                                                                    // verifier sent at registration
```

- `salt_srp` is random and independent of `kdf_salt` (as in ADR-010).
- The server stores `(account_id, salt_srp, v)` and never sees the password, the Secret Key, the
  URK, MK/MEK, or VK.
- Because `x` is a function of the URK, a stolen verifier `v` is offline-crackable **only** by an
  attacker who also has the 128-bit Secret Key — restoring the 1Password guarantee to SRP.
- The SRP login flow (mutual auth, session key `K`, bearer token, channel binding) is otherwise
  exactly ADR-010 §login.

### 5. Account & session model (multi-user self-host)

Self-hosted mode gains real accounts and auth (no more anonymous `vault_id` access):

- **Roles:**
  - `admin` — first account created on a fresh server (Immich-style bootstrap); can create,
    disable, and delete user accounts, set per-user quotas, and view server health. Has **no**
    ability to read any user's plaintext (zero-knowledge holds for admins too).
  - `user` — owns one or more vaults; can sign in from any device with password + Secret Key.
- **Account record (server):** `account(id, email, role, srp_salt, srp_verifier, kdf_version,
  status, created_at)`. The legacy `api_token` bearer column is replaced by SRP-derived,
  short-lived session bearer tokens (ADR-010). `tier`/quota columns remain for hosted mode.
- **Sessions:** an SRP login yields session key `K`; the bearer token is HMAC-`K`-signed,
  short-lived, and refreshed by re-running SRP. Vault and event endpoints authorize on the
  session's `account_id` (replacing anonymous `vault_id` access in self-hosted mode).
- **Server modes:** `--mode hosted` keeps billing/quotas/tiers; `--mode self-hosted` now enables
  accounts + auth (previously 404). Both store only ciphertext + verifiers.

### 6. Vault-header changes

The vault header (architecture §5.2) gains the account binding and KDF version and **loses the
recovery-key fields**:

- **Add:** `account_id` (16 B, server-assigned UUID) and `kdf_version` (u16).
- **Keep:** `kdf_salt`, `hkdf_salt`, Argon2 params, `vk_wrap_nonce`, `vk_wrap_ct`.
- **Remove:** `vk_recover_nonce`, `vk_recover_ct`, and the `has_recovery` flag (no escrow
  recovery path remains).
- **Never stored:** the Secret Key (in any form) and the URK.

The header continues to be integrity-bound as AAD on the VK-unwrap AEAD, now covering
`account_id` and `kdf_version` so neither can be silently altered.

### 7. Emergency Kit & recovery (sole path)

There is **one** recovery path — the **Emergency Kit**, a printable/savable document containing:

- the **sign-in address** (server URL),
- the **account email**,
- the **Secret Key** (the `A4-…` string above),
- a space for the user to optionally write the password (1Password convention; Tock never stores it).

Recovery / new-device sign-in = password + Secret Key → URK → MEK → unwrap VK → pull & decrypt.
There is **no** server-side reset and **no** VK escrow. **Losing the Secret Key with no Emergency
Kit means the data is unrecoverable — by design.** This is stated plainly to users at account
creation, and the Emergency Kit is presented (with a "save/print now" gate) before onboarding
completes.

### 8. Onboarding a new device (Secret Key sign-in as primary)

Secret Key sign-in **replaces device pairing as the primary** new-device path:

```text
New device N                                            Server S
────────────                                            ────────
1. User enters sign-in address + email
2. User enters password and Secret Key
   (typed, or scanned from Emergency Kit Setup Code / QR)
3. N runs SRP-6a login (x from URK) ───────────────────► verify; issue session bearer
4. N derives URK → MEK; GETs vault header + wrapped VK ◄─ ciphertext only
5. N unwraps VK with MEK; pulls snapshot + events; decrypts; materializes state
6. N stores Secret Key in the platform keystore; publishes Create(Device)
```

Device-to-device **pairing** (QR + X25519 VK transfer, architecture §6.5,
[ADR-003](ADR-003-event-sourced-sync.md)) is **retained as an optional convenience** (e.g. to
avoid retyping the Secret Key when an existing trusted device is present) but is no longer the
primary path and is not required for onboarding.

## Consequences

**Positive:**

- **1Password-grade resistance to server compromise:** a stolen server DB (SRP verifier +
  ciphertext) is uncrackable without the 128-bit Secret Key, regardless of password strength.
- **Real self-hosted accounts & auth:** Immich-style admin + N users, with per-account
  authorization replacing anonymous `vault_id` access — while the server stays zero-knowledge.
- **Sign-in from any device** with password + Secret Key, no existing device required; pairing
  becomes a convenience, not a dependency.
- **Simpler recovery story:** exactly one path (Emergency Kit); no escrow key to leak or rotate.
- **Forward-compatible:** `kdf_version` allows Argon2id/KDF upgrades via re-wrap, no data loss.

**Negative:**

- **No recovery without the Secret Key.** Users who lose both the Secret Key and the Emergency
  Kit lose their data permanently. This is a deliberate, clearly communicated trade-off.
- **Higher onboarding friction:** users must safeguard and (sometimes) transcribe a 128-bit
  Secret Key; mitigated by Setup Code/QR scanning and keystore caching after first sign-in.
- **Implementation surface grows:** 2SKD + SRP-6a in `tock-crypto`, account/role/session schema
  and auth middleware in `tock-server`, and onboarding/Emergency-Kit UX in clients. Tracked by
  dependent issues.

**Neutral:**

- The vault hierarchy below MEK (VK → DK → IK, per-item AEAD, padding, AAD) is unchanged — only
  the **root** of the hierarchy and the **SRP input** move from password to URK.
- [ADR-003](ADR-003-event-sourced-sync.md) is **not superseded**: event-sourced sync and the
  pairing transport remain; only the *primary onboarding flow* changes.
- Relies on the same audited crates plus a maintained SRP-6a implementation (per ADR-010).

## Implementation impact (downstream, not in this ADR)

- `tock-crypto`: add 2SKD (`URK = K_pw XOR K_sk`), Secret Key generation/encoding/checksum,
  SRP-6a (`x` from URK, verifier, login), `kdf_version` plumbing.
- `tock-core` / `tock-storage`: vault header v2 (`account_id`, `kdf_version`; drop `vk_recover_*`),
  URK-rooted `KeyHierarchy`, header AAD coverage.
- `tock-server`: account/role/session schema, SRP endpoints, self-hosted auth middleware, admin
  bootstrap, per-account authorization. **Branch-protection / CI note:** new server routes don't
  change CI job names, but any new merge-gate jobs must be mirrored in `kafkade/github-infra`
  (`repo_tock.tf`).
- Clients: password + Secret Key onboarding, Emergency Kit generation + "save/print" gate,
  keystore caching of the Secret Key.
