# Self-hosting tock

Run your own tock instance and connect the CLI and apps to it. Everything is
**end-to-end encrypted** — the server only ever stores opaque ciphertext, and
your password and Secret Key never leave your devices
([ADR-002](adr/ADR-002-end-to-end-encryption.md)).

You copy the compose file, set a `.env`, `docker compose up -d`, create an
admin account + Emergency Kit in the browser, and point a client at your
instance. That's the whole loop.

> **Licensing.** `tock-server` is **AGPL-3.0-only**
> ([ADR-006](adr/ADR-006-licensing-dual-license.md)); its source is this
> repository. The web console (`apps/web`) is Apache-2.0 and talks to the
> server only over HTTP, so it ships as a **separate container** — it is a
> separate work, not a derivative of the server.

## Contents

- [Requirements](#requirements)
- [1. Quick start (one command)](#1-quick-start-one-command)
- [2. First-run setup + Emergency Kit](#2-first-run-setup--emergency-kit)
- [3. Enable HTTPS (TLS)](#3-enable-https-tls)
- [4. Connect the CLI](#4-connect-the-cli)
- [5. Connect the iOS / macOS apps](#5-connect-the-ios--macos-apps)
- [6. Managing users](#6-managing-users)
- [7. Self-service (your account)](#7-self-service-your-account)
- [8. Operations](#8-operations)
- [How your data stays private](#how-your-data-stays-private)

## Requirements

- A host with **Docker** and the **Docker Compose** plugin.
- For HTTPS: a **domain name** whose DNS points at the host, with ports **80**
  and **443** reachable from the internet.
- tock uses only **SQLite** on a data volume — no Postgres, no Redis.

## 1. Quick start (one command)

From a checkout of this repository:

```sh
cp .env.example .env
# edit .env — at minimum review TOCK_HTTP_PORT and TOCK_REGISTRATION_POLICY
docker compose up -d
```

This builds and starts two containers:

| Service       | Role                                                               |
| ------------- | ------------------------------------------------------------------ |
| `tock-server` | Encrypted blob store + account/auth API (AGPL-3.0). Not published. |
| `tock-web`    | Web console + first-run wizard; serves the UI and proxies the API. |

`tock-web` is your single entrypoint on `http://<host>:${TOCK_HTTP_PORT}`
(default `8080`). Confirm the stack is healthy:

```sh
docker compose ps
curl -f http://localhost:8080/health   # -> {"status":"ok"}
```

### Key `.env` settings

| Variable                   | Purpose                                                     |
| -------------------------- | ----------------------------------------------------------- |
| `TOCK_HTTP_PORT`           | Host port for the plain-HTTP entrypoint (default `8080`).   |
| `TOCK_PUBLIC_URL`          | The URL users connect to (docs/output only).                |
| `TOCK_MODE`                | `self-hosted` (default) or `hosted`.                        |
| `TOCK_REGISTRATION_POLICY` | `disabled` (default), `invite-only`, or `open`.             |
| `TOCK_ADMIN_USERNAME`      | Optional headless admin bootstrap (see below).              |
| `TOCK_DOMAIN` / `TOCK_ACME_EMAIL` | Domain + email for the optional TLS profile.         |

The default `disabled` policy means **no one can self-register** — the first
account is bootstrapped as admin, and everyone else joins by invite. You can
change the policy any time from the admin console.

## 2. First-run setup + Emergency Kit

Open `http://<host>:${TOCK_HTTP_PORT}` in a browser. Because no account exists
yet, the console shows the **first-run wizard**:

1. Enter an **admin email** and **password**. This first account is
   automatically the **administrator**.
2. The wizard shows your **Emergency Kit** (your Secret Key + recovery info)
   and a **Setup Code**. **Save the Emergency Kit now** — print it or store it
   in a password manager. It is shown **once**, and *nobody, including the
   server, can recover it for you*. You need your password **and** Secret Key
   to sign in on a new device.
3. Choose the **registration policy** and **public server address** (the base
   URL clients should use to reach this instance — pre-filled with the current
   origin; both are editable later in the console).
4. Tick "I have saved my Emergency Kit" and continue into the admin console.

### Headless / CLI bootstrap (alternative)

If you'd rather not use the browser wizard, bootstrap the admin from the host:

```sh
# On a fresh instance, mint an admin invite and print a setup token:
docker compose exec tock-server tock-server admin create-admin --username you@example.com
```

Or set `TOCK_ADMIN_USERNAME` in `.env` before first start — the server mints
the invite on boot and logs the setup token (`docker compose logs tock-server`).
Redeem the token by registering that username from any tock client.

## 3. Enable HTTPS (TLS)

For anything beyond localhost, terminate TLS. The stack bundles **Caddy** with
automatic Let's Encrypt certificates. In `.env` set:

```sh
TOCK_DOMAIN=tock.example.com
TOCK_ACME_EMAIL=you@example.com
TOCK_PUBLIC_URL=https://tock.example.com
```

Point `tock.example.com` at your host, open ports 80 + 443, then:

```sh
docker compose --profile tls up -d
```

Caddy fetches and renews the certificate automatically and serves
`https://tock.example.com`.

Prefer nginx or Traefik? See [`deploy/proxy/`](../deploy/proxy/README.md) for
drop-in samples. All three terminate TLS and forward to `tock-web`.

## 4. Connect the CLI

Install the `tock` CLI (see the [README](../README.md)), then create or link an
account against **your** instance:

```sh
# Create an account on your instance (prompts for a password):
tock account signup --server https://tock.example.com --email you@example.com

# ...or, on another device, sign in with your Secret Key / Setup Code:
tock account login --server https://tock.example.com --email you@example.com
# equivalently, paste the Setup Code from the wizard:
tock account login --setup-code "TOCK1:…"

tock account status          # shows who you're signed in as, and where
```

The server URL is persisted after the first use, so day-to-day you just sync:

```sh
tock sync                    # push/pull encrypted events to your instance
```

See [`docs/dogfooding.md`](dogfooding.md) for a full two-device round-trip
walkthrough (including how conflicts surface).

## 5. Connect the iOS / macOS apps

The Apple apps use the same account model over
[UniFFI](adr/ADR-005-platform-bindings.md). To point an app at your instance,
enter your server URL (e.g. `https://tock.example.com`) during onboarding, or
paste the **Setup Code** from the first-run wizard / `tock account` output —
it bundles the server URL, email, and Secret Key so the app is configured in
one step.

> The SwiftUI onboarding flow is still landing; until then, use the CLI (above)
> or the web console to drive your instance. The account/auth binding it builds
> on is already in place.

## 6. Managing users

Sign in to the console as the admin (`https://tock.example.com`). The
**Admin console** lets you:

- **Set the registration policy** — `open`, `invite-only`, or `disabled`.
- **Set the public server address** — the base URL shown to users adding a new
  device; editable at any time.
- **Invite users** — admins can't set passwords (zero-knowledge), so "adding a
  user" mints an **invite token** the user redeems when they register with
  their own client-computed credentials.
- **Enable / disable / delete** accounts.
- **See usage & health** — an at-a-glance panel with account counts (by role and
  status), vault / device / event totals, encrypted storage size, and a live
  `/health` check.

The same operations are available offline from the host:

```sh
docker compose exec tock-server tock-server admin list-users
docker compose exec tock-server tock-server admin reset-registration --policy invite-only
```

## 7. Self-service (your account)

Any signed-in (non-admin) user has a **Your account** portal, reachable from the
task view, that keeps account management in the browser — no CLI required. It is
strictly self-scoped: you can only ever see or change your own resources.

- **Change your password** — rotation happens **entirely in your browser**. Your
  new password re-derives your Unlock Root Key, re-wraps your Vault Key, and
  mints a fresh SRP verifier in WASM; only the non-secret verifier material and
  the re-wrapped (still encrypted) vault header are uploaded. Your **Secret Key
  and Emergency Kit are unchanged** — you only need the new password on each
  device. After rotating, other devices must sign in again with the new
  password; use "sign out all other sessions" to force that.
- **Add another device** — re-display your **Setup Code** (with QR + print) on
  demand. It is derived locally from the server address, your email, and your
  Secret Key, so it never touches the server.
- **Manage devices** — list your registered devices and revoke any that should
  no longer sync.
- **Manage sessions** — list your live sessions (the current one is flagged),
  revoke an individual session, or end every other session at once.

Rotation and Setup-Code regeneration need your **Secret Key**, which the browser
holds **only in memory** for the current session (never on disk). If you reload
the page, sign in again to use them.

## 8. Operations

### Backup & restore

The data volume holds the **only copy** of your encrypted event store — back it
up. With the default named volume `tock-data`:

```sh
# Backup: stop for a consistent snapshot, tar the volume, restart.
docker compose stop tock-server
docker run --rm -v tock-data:/data -v "$PWD":/backup alpine \
  tar czf /backup/tock-backup-$(date +%F).tar.gz -C /data .
docker compose start tock-server

# Restore into a fresh volume:
docker run --rm -v tock-data:/data -v "$PWD":/backup alpine \
  sh -c "rm -rf /data/* && tar xzf /backup/tock-backup-YYYY-MM-DD.tar.gz -C /data"
```

The backup is ciphertext only; keep your Emergency Kit separately — without it
the data cannot be decrypted.

### Upgrades

Pin a version by setting `TOCK_SERVER_IMAGE` / `TOCK_WEB_IMAGE` in `.env`
(e.g. `ghcr.io/kafkade/tock-server:v0.5.0`), then:

```sh
docker compose pull        # or `docker compose build` if building from source
docker compose up -d
```

The SQLite schema migrates automatically on start. Back up the volume first.

### Health & metrics

- `GET /health` — liveness probe (used by the container healthcheck).
- `GET /metrics` — JSON counters for scraping/monitoring.
- `GET /v1/server/info` — public instance metadata (`setup_required`,
  registration policy, mode, version, and the configured public address); this
  is what the console uses to decide whether to show the first-run wizard.
- `GET /v1/admin/stats` — admin-only aggregate usage counters (accounts by
  role/status, vaults, devices, events, encrypted storage bytes); this backs the
  console's usage & health panel.

```sh
curl -f https://tock.example.com/health
curl -s https://tock.example.com/v1/server/info | jq
```

## How your data stays private

The server is an **encrypted blob store**: it stores and serves ciphertext and
never holds your password, Secret Key, or vault key. Registration uses
[SRP](adr/ADR-010-srp-authentication.md) so the server verifies you without
ever receiving your password, and every task/habit/event is encrypted client
side ([ADR-002](adr/ADR-002-end-to-end-encryption.md),
[ADR-011](adr/ADR-011-account-based-self-host-two-secret-auth.md)). That's why
losing your Emergency Kit is unrecoverable — and why self-hosting gives you the
data without giving the server your secrets.
