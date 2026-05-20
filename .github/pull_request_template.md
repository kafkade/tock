## Description

<!-- What does this PR do? Provide a brief summary of the changes. -->

## Related Issues

<!-- Link related issues: "Closes #123" or "Relates to #456" -->

## Type of Change

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Documentation update
- [ ] Refactoring (no functional changes)
- [ ] CI / infrastructure
- [ ] Other (describe below)

## Component

<!-- Which part of the monorepo does this touch? -->

- [ ] `crates/tock-core/` — Core library (domain model, crypto, storage traits)
- [ ] `crates/tock-cli/` — CLI tool (clap + ratatui TUI)
- [ ] `crates/tock-server/` — Sync server (Axum, AGPL-3.0)
- [ ] `crates/tock-crypto/` — Encryption primitives
- [ ] `crates/tock-sync/` — Sync protocol and transport
- [ ] `bindings/swift/` — UniFFI Swift bindings
- [ ] `apps/ios/` — iOS / iPadOS / macOS / watchOS app
- [ ] `apps/web/` — Web app (WASM)
- [ ] `docs/` — Documentation

## Domain

<!-- Which productivity domain(s) does this touch? -->

- [ ] Task management
- [ ] Habit tracking
- [ ] Time tracking
- [ ] Focus timer (Pomodoro)
- [ ] Cross-domain integration

## Privacy Checklist

<!-- All changes must uphold zero-knowledge principles -->

- [ ] No plaintext user data is sent to or stored on the server
- [ ] No new metadata exposure introduced (or documented if unavoidable)
- [ ] Any new external service interaction is documented in the trust boundary model
- [ ] Crypto changes use audited RustCrypto crates — no custom primitives
- [ ] Key material is never exposed in error messages, logs, or Debug output

## Checklist

- [ ] I have read [CONTRIBUTING.md](CONTRIBUTING.md)
- [ ] Tests pass (`cargo test --workspace`)
- [ ] Clippy passes (`cargo clippy --workspace -- -D warnings`)
- [ ] Formatting passes (`cargo fmt --check`)
- [ ] I have updated documentation (if applicable)
