# ADR-013: Vault format versioning & 1.0 compatibility policy

**Status:** Accepted
**Date:** 2026-07-02

> **Amends [ADR-011](ADR-011-account-based-self-host-two-secret-auth.md)** by
> ratifying the vault-format break it introduced (`v1` password-only →
> `v2` two-secret) as the **final pre-1.0 break**, and by committing to a
> **forward** compatibility guarantee for the `v2` format from 1.0 onward.
> Complements the schema-migration rules in
> [ADR-004](ADR-004-sqlite-app-layer-encryption.md) and `docs/architecture.md`
> §3 / §5.2.

## Context

The vault header (`tock-core/src/vault/header.rs`) carries a `format_version`.
[ADR-011](ADR-011-account-based-self-host-two-secret-auth.md)'s two-secret key
derivation (2SKD) bumped it from `1` to `2`: the header gained `account_id` and
`kdf_version`, and the key hierarchy now roots in the Unlock Root Key (password
**and** Secret Key) instead of the password alone. Because a `v1` vault has no
Secret Key and no account binding, a `v2` build cannot derive its keys. The code
detects this and refuses the vault with a clear, dedicated error rather than a
confusing missing-field failure:

```text
FORMAT_VERSION      = 2   // what this build writes
MIN_COMPAT_VERSION  = 2   // lowest format this build will open
```

```rust
// tock-core/src/vault/header.rs — VaultHeader::from_meta
if format_version < MIN_COMPAT_VERSION {
    return Err(Error::VaultNeedsReinit { found: format_version });
}
```

```text
vault uses the legacy password-only format (v1); re-initialize it
(no automatic migration before 1.0)
```

This "re-init, no migration" behavior has been acceptable because **pre-1.0
semver permits breaking changes** and no official/stable release ever shipped
`v1` to end users — only pre-#126 dogfooders hold `v1` vaults. But **1.0 is a
stability commitment**. Before cutting the first official release we need an
explicit, written policy stating (1) exactly what happens to existing vaults at
1.0 and (2) what format-stability guarantee 1.0 makes going forward. This ADR is
that policy. See issue #171.

Two format-evolution levers already exist and shape the policy:

- **`kdf_version`** — selects Argon2id parameters and the 2SKD/HKDF `info`
  labels. Bumping it is a *forward-compatible re-wrap* (re-derive URK, re-wrap
  MEK→VK, push a new SRP verifier) with **no data loss** — the vault stays
  `v2`. This is the intended path for KDF hardening.
- **`min_compatible_version`** — older clients refuse to open newer vaults
  (`Error::UnsupportedVaultVersion`); this protects against silent downgrades.

## Decision

### 1. v1 → v2: no migration; re-initialization required (final pre-1.0 break)

We **ratify** "no automatic migration; re-initialize" as the **final** pre-1.0
vault-format break. At and after 1.0 there is no code path that opens or
upgrades a `v1` password-only vault; `MIN_COMPAT_VERSION` stays `2`.

We deliberately do **not** ship a `tock migrate` command, because:

- No official release ever distributed `v1`; the affected population is a
  handful of pre-#126 dogfooders.
- Data is not trapped. tock is local-first with JSON export/import
  (`tock-export` / `tock-import`), so the upgrade path is a lossless
  **export → re-init → import**.
- 2SKD mandates a freshly generated Secret Key and a new Emergency Kit anyway,
  so a "migration" would still force new-account onboarding — it would not be
  meaningfully smoother than re-init, while adding permanent maintenance
  surface and a second, rarely exercised crypto path to keep correct.

**One-time upgrade note (documented for users):** if `tock` reports the
legacy-format error, export from the old build, re-initialize with 1.0
(`tock account signup` / `tock init`), and re-import. Store the new Emergency
Kit and Secret Key safely.

### 2. Forward compatibility guarantee for v2 (from 1.0 onward)

From 1.0, tock makes the following promises about the `v2` vault format:

- **Readability within 1.x.** Any `v2` vault written by a 1.x build opens on
  every later 1.x build. `MIN_COMPAT_VERSION` will **not** be raised above `2`
  within the 1.x line; 1.x never orphans a 1.x-written vault.
- **No silent data-destroying breaks.** No release removes the ability to open a
  supported vault without providing an in-place, automatic migration that runs
  transparently on unlock and preserves all data. Users are never asked to
  re-initialize or re-enter data to keep using an existing vault within a major
  version.
- **KDF/parameter hardening is transparent.** Strengthening Argon2id or changing
  HKDF labels happens via a **`kdf_version`** bump — a forward-compatible re-wrap
  performed on unlock. The vault stays `v2`; no user action, no re-init.
- **Structural format changes migrate in place.** A future `format_version`
  bump (e.g. `v3`) ships with an automatic on-open migration and only occurs at
  a major version boundary, consistent with the additive-within-a-major rule in
  `docs/architecture.md` §3. `min_compatible_version` is set so older clients
  fail closed (refuse) rather than misread a newer vault.
- **Downgrade safety.** Older clients continue to refuse newer vaults via
  `min_compatible_version` (`Error::UnsupportedVaultVersion`) instead of
  corrupting them.

In short: **1.0 draws the line at `v2`.** Everything from `v2` forward is either
readable as-is or migrated automatically with no data loss; the only vaults ever
requiring manual re-init are the pre-1.0 `v1` vaults covered by §1.

## Consequences

**Positive:**

- A single, written, released policy satisfies the 1.0 stability commitment and
  closes issue #171's acceptance criteria.
- One crypto/unlock path to maintain (`v2` only); no dual-format decode logic.
- Clear, bounded promise for users and self-hosters: existing `v2` vaults keep
  working; KDF upgrades are invisible; only the tiny pre-1.0 `v1` cohort re-inits.

**Negative:**

- Pre-#126 dogfooders with `v1` vaults must export → re-init → import once. This
  is a one-time, clearly documented cost affecting a very small group.
- Committing to automatic in-place migration for any future structural change is
  a real engineering obligation on future format work (no "just re-init"
  shortcut after 1.0).

**Neutral:**

- No code change is required to enact this decision — current behavior
  (`MIN_COMPAT_VERSION = 2`, `VaultNeedsReinit`) already implements §1; this ADR
  makes it policy and adds the §2 forward guarantee that future changes must
  honor.
- The `kdf_version` re-wrap mechanism (ADR-011) becomes the standard, documented
  tool for parameter evolution without a format break.
