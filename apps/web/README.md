# apps/web

React + TypeScript + Vite web client. Account onboarding (signup, login, and
authenticated sync) runs entirely in the browser via the `tock-wasm`
(wasm-bindgen) binding over `tock-account`. HTTP lives at this edge per ADR-001;
the SRP-6a / 2SKD math runs in WASM. See ADR-012.

## Develop

```sh
# 1. Build the WASM package the app depends on (file:../../crates/tock-wasm/pkg)
npm run wasm

# 2. Install deps and run the dev server
npm install
npm run dev
```

## Build & test

```sh
npm run build   # tsc -b && vite build
npm test        # vitest
```

## Notes

- Credentials (bearer token + channel binding) default to **in-memory**;
  a `sessionStorage` tier is opt-in. The account Secret Key is **never**
  persisted in the browser — re-enter it or paste a Setup Code on a fresh tab.
- No browser-local SQLite vault yet, so the task page is an auth smoke check;
  full task CRUD over the WASM core is a follow-up.

## Admin console & first-run wizard

The app doubles as the **self-host admin console**. On load it fetches
`GET /v1/server/info`:

- **Fresh instance** (`setup_required: true`) → a first-run wizard creates the
  **admin** account (the first registrant is bootstrapped as admin), shows the
  Emergency Kit + Setup Code (with a save gate), then opens the console.
- **Existing instance** → sign in. Admin sessions land in the console
  (user management + registration policy); everyone else lands in the task
  view. Admin rights are detected by probing `GET /v1/admin/settings`.

Admins cannot set passwords (zero-knowledge): "adding a user" mints an invite
the user redeems with their own client-computed SRP credentials.

### Production serving

In production the console is served as a **separate container** (nginx static
build, see `../../Dockerfile.web`) behind the reverse proxy, which routes
`/v1`, `/health`, `/metrics` → `tock-server` and everything else → this SPA.
Because the SPA talks to the AGPL server only over HTTP, it stays a separate
Apache-2.0 work (see the self-hosting guide and ADR-006). The API is therefore
**same-origin** (base `""`); in dev, `vite.config.ts` proxies the same paths to
`TOCK_SERVER_PROXY` (default `http://localhost:8787`) so behaviour matches.
