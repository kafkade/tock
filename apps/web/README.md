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
