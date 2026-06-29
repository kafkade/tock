# ADR-012: Client account onboarding — Emergency Kit, Setup Code, shared orchestration

**Status:** Accepted
**Date:** 2026-06-29

## Context

ADR-010/011 established server-side SRP-6a auth and the two-secret key
derivation (2SKD): the Unlock Root Key needs **both** a password and a
client-only Secret Key. The server (ADR-001) stores only ciphertext, opaque
events, and an account/verifier — never the password, Secret Key, or any key
material.

What was missing was the *client* onboarding: turning the merged crypto
(#126/#127/#128) and server endpoints (#120/#122/#130) into a usable signup /
sign-in / add-device flow across the CLI, Apple apps, and a web app. Three
edges, three HTTP stacks, one wire protocol — duplicating the SRP and signup
derivation per client would invite drift and bugs in security-critical code.

Two recovery artifacts are needed (1Password model): a durable **Emergency
Kit** for cold recovery, and a fast **Setup Code** for adding a trusted device.
A fresh device with no peer online has a chicken-and-egg: it needs the KDF salt
to derive the URK, but the salt lives in the vault header it cannot yet read.

## Decision

- A new zero-I/O crate, **`tock-account`**, owns all account orchestration:
  signup derivation, the SRP login state machine, `KdfParams`, credential
  structs, and the Emergency Kit / Setup Code codecs. It wraps `tock-crypto` /
  `tock-core` and produces plain request/response structs. **No HTTP** — each
  client edge posts them (CLI/reqwest, Apple/URLSession via UniFFI, web/fetch
  via WASM). One protocol, one test surface.
- **Emergency Kit**: text + printable PDF carrying server URL, email, and the
  `A4-…` Secret Key. **Setup Code**: `TOCK1:<base64-json>` text plus a QR.
  Neither contains the password.
- **New-device bootstrap**: `srp/start` returns `kdf_params`; the client derives
  its URK pre-auth, logs in, then `GET /v1/vaults/:id/header` recovers the
  wrapped VK. The server stores the header at signup (`PUT …/header`); it stays
  non-secret because the VK is wrapped under MEK←URK.
- **Credentials**: Secret Key + bearer token + channel binding + expiry, per
  platform (OS keyring / Keychain / IndexedDB). The password is never stored.

## Consequences

**Positive:**

- Security-critical SRP/2SKD logic lives once and is reused by every client.
- New device sign-in needs only email + password + Secret Key — no peer online.
- Server remains zero-knowledge; the stored header leaks nothing.

**Negative:**

- Adds client deps (keyring, QR, PDF) at the edges; none in the zero-I/O core.
- Web Secret-Key persistence is weaker than an OS keychain (in-memory default).
- SwiftUI onboarding screens land as a stacked follow-up.

## Status

Implemented: shared `tock-account` orchestration, server vault-header
bootstrap, CLI `tock account` + authed transport, UniFFI account API, the
`tock-wasm` web binding, and a React + Vite web app (`apps/web`) with
signup → Emergency Kit / Setup Code → login → authed sync. Web credentials
default to in-memory (sessionStorage opt-in); the Secret Key is never persisted
in the browser. SwiftUI signup/sign-in views remain a follow-up.
