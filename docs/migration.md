# Migrating servers & downloading your data

Tock is **local-first** and **end-to-end encrypted**. Your device holds the
authoritative copy of your data; a sync server is an optional, zero-knowledge
relay that only ever stores ciphertext
([ADR-002](adr/ADR-002-end-to-end-encryption.md)). That design has a practical
consequence worth stating plainly:

> **You can leave at any time, and nothing about leaving is a special favour.**
> Moving between hosted tock, your own server, and no server at all is a
> supported, first-class path — not an export button bolted on as an afterthought.

This guide walks the two stories people actually ask about: *"can I move my
account to a different server?"* and *"can I download my data?"*

## Contents

- [What you can take with you](#what-you-can-take-with-you)
- [Move my account to another server](#move-my-account-to-another-server)
- [One server at a time (the authoritative-server invariant)](#one-server-at-a-time-the-authoritative-server-invariant)
- [Download my data](#download-my-data)
- [My server is gone](#my-server-is-gone)
- [Which artifact for which job](#which-artifact-for-which-job)
- [Why this matters](#why-this-matters)
- [See also](#see-also)

## What you can take with you

Four different artifacts exist, and they are genuinely different things. Picking
the wrong one is the most common mistake.

| Artifact | Command | Encrypted? | What it's for |
| --- | --- | --- | --- |
| **Server ciphertext archive** | `tock account export` | Yes (unchanged E2EE ciphertext) | Portability + the second half of a server migration |
| **Client vault backup** | `tock backup create` | Yes (outer AES-256-GCM) | Restoring a vault after losing a device |
| **Plaintext export** | `tock export json` / `tock export md` | **No** | Reading your data in other tools |
| **Your local vault** | the `.tockvault` file | No (plaintext at rest) | Nothing — don't copy it; use `tock backup create` |

Two of those are ciphertext, and ciphertext is only useful with the key. **Your
Emergency Kit — your Secret Key, plus your password — is what turns any encrypted
artifact back into your data.** Store it separately from the archives themselves.
There is no recovery path around it, by design
([ADR-011](adr/ADR-011-account-based-self-host-two-secret-auth.md)).

## Move my account to another server

Moving keeps your **crypto identity** intact: the client-minted `account_id`
(**A**) and `vault_id` (**V**) are yours, minted on your device before any server
existed, and they pass through the move byte-for-byte. The new server mints only
its own bookkeeping principal (**B**). No key rotation, no re-encryption, no
re-derivation ([ADR-016](adr/ADR-016-three-id-identity-and-adoption.md) §1).

From the device that currently holds the vault:

```sh
# 1. Download the ciphertext archive from your CURRENT server.
tock account export --out vault.json

# 2. Move the binding to the new server. This disconnects the old one first.
tock account adopt --server https://new.example.com --email you@example.com --migrate

# 3. Upload the archive to the NEW server.
tock account import vault.json

# 4. Reconcile this device with its new home.
tock sync
```

Then confirm from a *second* device, which is the real proof that the history
moved:

```sh
tock account login --server https://new.example.com --email you@example.com
tock sync
tock list       # your pre-migration tasks are all here
```

### Why the order is export → adopt → import

The sequence above is not arbitrary, and the intuitive "export, import, then
switch servers" reading does not work:

- **`import` must come after `adopt`.** Uploading an archive requires a live,
  channel-bound session *on the destination server*, and the only thing that
  creates an account there for an existing vault is `adopt`. (`tock account
  signup` is not a substitute — it refuses to run when a local vault already
  exists, because it would mint a brand-new **A** and **V**.)
- **`export` must come before `--migrate`.** A migration is
  *disconnect-old-then-adopt-new*: `--migrate`'s first act is to revoke your
  principal on the old server and clear its stored credentials
  ([ADR-016](adr/ADR-016-three-id-identity-and-adoption.md) §4). Once that has
  happened you can no longer authenticate to the old server to download anything.
- **`import` is not redundant.** `adopt` performs an initial push, but that push
  only carries *pending local changes* — deltas your device has not yet recorded
  as synced. A vault that was fully synced to the old server has none, so
  `adopt --migrate` uploads your wrapped vault header and an **empty event log**.
  Your own device keeps working (its local data never moved), but a *new* device
  signing in to the new server would find nothing. `import` is what actually
  carries the history across. `adopt --migrate` says so on completion — it prints
  a `Binding moved:` summary warning that the history did not follow and pointing
  at the remaining `import` + `sync` steps.

Step 4's `tock sync` is safe and idempotent: the migration reset your pull
cursor, so the client re-pulls the events it just imported, recognises every one
of them as already present locally, and reports zero conflicts.

## One server at a time (the authoritative-server invariant)

**A vault binds to exactly one server at a time.**

```sh
tock account adopt --server https://other.example.com --email you@example.com
# error: vault is already bound to https://old.example.com;
#        pass --migrate to move it to https://other.example.com
```

That refusal is deliberate. Without it the same `vault_id` could be pushed to two
servers whose histories would then diverge with no way to reconcile them —
split-brain. `--migrate` is the explicit, single-purpose escape hatch, and it is
transactional in spirit: the new `{server URL, principal, credentials}` is
written **only after** the new adoption succeeds, so a migration that fails
partway leaves your vault local-only rather than half-bound to two servers.

To leave a server without joining another:

```sh
tock account disconnect
```

`disconnect` revokes the server principal **B**, resets the sync cursor, and
clears stored credentials. **A**, **V**, and every task, habit, time block, and
focus session stay exactly as they are — the vault returns to a fully usable
local-only state and can be re-adopted later. It never rotates or re-wraps keys.

## Download my data

### The ciphertext archive (portability)

```sh
tock account export --out vault.json
```

This downloads a single JSON archive containing your non-secret vault header
plus your **entire event log**, exactly as the server stores it — as opaque
ciphertext. The server never decrypts anything to produce it, and it authorizes
the request with the same double check as every sync route (your session **and**
proven ownership of the vault), so no one else can pull your archive
([ADR-019](adr/ADR-019-server-retained-snapshots-and-pitr.md) §2).

To read that archive you need **your password and the Secret Key from your
Emergency Kit**. Keep the kit somewhere other than wherever you keep the archive;
together they are your data, and either alone is not.

What this archive is **not**:

- **Not human-readable.** Every payload is AEAD ciphertext.
- **Not a backup of your device.** It is what the *server* holds. Use
  `tock backup create` for a restorable client-side snapshot
  ([ADR-018](adr/ADR-018-backup-restore-format-and-modes.md)).
- **Not the server's disaster recovery.** A download does nothing if the server
  loses its database before you download. That is what server-retained snapshots
  are for — see [self-hosting](self-hosting.md#backup--restore).

### The encrypted client backup

```sh
tock backup create --out my-vault-$(date +%F).tockbak
```

An outer-encrypted, transactionally consistent snapshot of your local vault,
sealed under a key derived from your Vault Key. This is the artifact to keep if
what you want is "I can get my setup back". Restore with an explicit mode:

```sh
tock backup restore my-vault-YYYY-MM-DD.tockbak --mode disaster-recovery  # the device is gone
tock backup restore my-vault-YYYY-MM-DD.tockbak --mode clone              # a second, live device
```

Never copy the raw `.tockvault` file as a "backup": its materialized tables hold
**plaintext at rest** ([ADR-014](adr/ADR-014-at-rest-encryption-app-layer-aead.md)),
so a stray copy is a plaintext leak rather than a safe archive.

### The plaintext escape hatch

If you want your data in another tool, tock will hand it over in the clear:

```sh
tock export json --out my-tasks.json
tock export md --builtin task-list --out my-tasks.md
```

> **Warning — this output is unencrypted.** `tock export` writes plaintext.
> Anyone who can read the file can read your data; no password, no Secret Key,
> and no Emergency Kit are needed. Treat the output like a secret: don't drop it
> in a shared folder, a synced cloud drive, or a bug report. It is a
> **portability** format, **not a backup** — you cannot restore a vault from it,
> and it carries none of your sync or device state.

The escape hatch exists on purpose. A privacy-first tool that made it hard to get
your data out in a form other software can read would be holding you hostage with
extra steps.

## My server is gone

Your clients hold the authoritative copy, so a lost server is an inconvenience,
not a data-loss event.

- **You self-host and the database is intact** — nothing to do.
- **You self-host and the database is lost** — restore the newest server-retained
  snapshot (they are produced automatically, daily by default) and restart. See
  [self-hosting → Backup & restore](self-hosting.md#backup--restore).
- **The server is rebuilt empty, or you're standing up a replacement** — from a
  device that still has the vault, re-adopt it and re-upload your history:

  ```sh
  tock account adopt --server https://new.example.com --email you@example.com --migrate
  tock account import vault.json   # a previously downloaded archive
  tock sync
  ```

- **The server is gone for good and you don't want another one** — run
  `tock account disconnect`. Your vault keeps working, offline and local-only,
  indefinitely.

Note the asymmetry that makes this safe: losing a *server* costs you at most the
events one device pushed but no device pulled. Losing your *Emergency Kit* is
unrecoverable. Guard the kit, not the server.

## Which artifact for which job

| I want to… | Use | Notes |
| --- | --- | --- |
| Move to a different server | `account export` → `adopt --migrate` → `account import` | Preserves **A** and **V** |
| Stop using a server entirely | `account disconnect` | Vault stays fully usable, local-only |
| Hold a copy of what the server has | `account export` | Ciphertext; needs the Emergency Kit |
| Be able to restore my vault | `backup create` / `backup restore` | Encrypted; explicit restore mode |
| Read my data in another tool | `tock export json` / `tock export md` | **Plaintext — handle with care** |
| Survive losing the server's database | Server-retained snapshots (operator) | See [self-hosting](self-hosting.md#backup--restore) |

Keep these distinct. In particular, **export ≠ import ≠ server-retained
snapshot**: a download is portability, an upload is restore, and only a retained
snapshot protects data the moment the server's database dies.

## Why this matters

Tock's business model is open core: every line of client code is Apache-2.0, the
sync server is AGPL-3.0 and free to self-host forever, and the paid offering is
*hosting* — not features, not your data
([ADR-007](adr/ADR-007-monetization-open-core.md),
[ADR-006](adr/ADR-006-licensing-dual-license.md)). As ADR-007 puts it: **"All code
is open source. Self-hosting is free forever. You're paying for hosting, not for
code."**

A model like that only earns trust if leaving is real. So it is:

- **No feature gating.** Free features never become paid ones.
- **No data hostage.** The server cannot read your data even if it wanted to, and
  you can download everything it holds with one command.
- **No dead end.** If the hosted service disappears tomorrow, `adopt --migrate`
  onto your own box, or `disconnect` and keep working offline. The CLI is fully
  functional with no server at all.

The migration path in this document is the concrete answer to "what if you shut
down?" — and it is tested end to end (`crates/tock-cli/tests/e2e_migration.rs`)
rather than merely asserted.

## See also

- [Self-hosting guide](self-hosting.md) — running your own server, and the
  operator side of backup & restore
- [Dogfooding guide](dogfooding.md) — multi-device sync setup
- [ADR-006](adr/ADR-006-licensing-dual-license.md) — licensing (Apache-2.0 +
  AGPL-3.0)
- [ADR-007](adr/ADR-007-monetization-open-core.md) — open core, and the
  no-lock-in commitment
- [ADR-016](adr/ADR-016-three-id-identity-and-adoption.md) — the three-ID identity
  model, adoption, `--migrate`, and `disconnect`
- [ADR-018](adr/ADR-018-backup-restore-format-and-modes.md) — client backup format
  and restore modes
- [ADR-019](adr/ADR-019-server-retained-snapshots-and-pitr.md) — server export,
  import, and retained snapshots
