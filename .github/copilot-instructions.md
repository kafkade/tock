# Copilot Instructions for tock

## Project Overview

tock is a unified personal productivity engine that fuses four traditionally separate tools — task management, habit tracking, time tracking, and a focus (Pomodoro) timer — into a single end-to-end encrypted, local-first system. It targets power productivity practitioners: developers, researchers, founders, and knowledge workers who have outgrown single-purpose apps.

**Stack**: Rust core library + CLI (clap/ratatui TUI) + iOS/iPadOS/macOS/watchOS (SwiftUI via UniFFI) + Web (WASM via wasm-bindgen) + optional sync server (Axum, AGPL-3.0).

## Architecture

### Monorepo Layout

- `crates/tock-core/` — Shared Rust library: domain model (tasks, habits, time blocks, focus sessions), query language, urgency scoring, recurrence, conflict resolution. **No I/O, no networking** — pure computation only.
- `crates/tock-crypto/` — Envelope encryption (AES-256-GCM), Argon2id KDF, key hierarchy, recovery keys. No I/O — pure crypto.
- `crates/tock-parse/` — Query language parser and filter engine.
- `crates/tock-storage/` — SQLite storage implementation (rusqlite). Implements storage traits from tock-core.
- `crates/tock-sync/` — Sync protocol: event-sourced, batch-encrypted, conflict resolution with user review.
- `crates/tock-import/` — Import from Todoist, Things 3, Toggl, hledger timeclock, CSV.
- `crates/tock-export/` — Export to JSON, CSV, hledger timeclock, iCal.
- `crates/tock-cli/` — CLI binary (clap commands + ratatui TUI). Only place in Rust that does HTTP (via reqwest).
- `crates/tock-server/` — Sync server (Axum). **AGPL-3.0 licensed** (has its own LICENSE file). Encrypted blob store — never decrypts user data.
- `crates/tock-uniffi/` — UniFFI facade for Apple platforms.
- `bindings/swift/` — UniFFI-generated Swift bindings + idiomatic async/await wrapper layer.
- `apps/ios/` — SwiftUI app consuming the Swift bindings.
- `apps/web/` — Web app consuming tock-core via WASM.

### Key Design Constraints

1. **tock-core must have zero I/O dependencies.** No `reqwest`, no file system access, no platform APIs. All I/O happens in platform-specific code. This keeps the core testable (pure functions) and compilable to WASM.

2. **Four unified domains.** Tasks, habits, time tracking, and focus sessions share IDs, events, and cross-domain primitives. A Pomodoro session linked to a task automatically logs a time block and increments a "deep work" habit. Use the unified query language across all four domains.

3. **Encryption is always client-side.** The server and sync transports only see encrypted blobs. If you're adding a feature that touches data at rest or in transit, it must go through the vault encryption layer.

4. **Methodology-neutral.** GTD views are available but not enforced. Identity-based habits are encouraged but not required. Never impose a single workflow.

5. **Sync is event-sourced.** Events are batch-encrypted (not individually) for performance. Conflicts on the same entity across devices require user review — no automatic last-write-wins for productivity data.

### Key Hierarchy (Crypto)

```
Password → Argon2id → MK → HKDF → MEK → wraps VK → wraps per-item IKs
```

- All key types implement `Zeroize`/`ZeroizeOnDrop`
- `Debug` impls must redact secret values
- Domain separation via AAD tags: `"tock-vault-wrap-v1"`, `"tock-item-wrap-v1"`, `"tock-recovery-wrap-v1"`
- Size-bucket padding before encryption (512B / 2KB / 8KB / 32KB)

## Conventions

### Error Handling

- Crypto failures must never expose key material in error messages
- Use `thiserror` for library errors in core crates
- Use `anyhow` only in binary crates (`tock-cli`, `tock-server`)
- Wrap errors with context (`anyhow::Context`)

### WASM Bundle Budget

The `core` WASM feature must stay under **2 MB compressed**. Feature flags in Cargo.toml control what's included:
- `core` = crypto + domain model + storage traits (always loaded)
- `sync`, `import-export` = lazy-loaded

### UniFFI / Swift

UniFFI generates callback-based APIs. The `bindings/swift/Sources/TockSwift/` wrapper layer converts these to idiomatic Swift async/await using `withCheckedThrowingContinuation`.

### Licensing

Everything is Apache-2.0 except `crates/tock-server/` which is AGPL-3.0. Don't move server code into tock-core or vice versa without considering license implications.

## Build & Test

```sh
# Build all crates
cargo build --workspace

# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p tock-core

# Run a single test by name
cargo test -p tock-core test_name

# Run tests for a specific module
cargo test -p tock-core tasks::

# Clippy lints
cargo clippy --workspace --all-targets -- -D warnings

# Format check
cargo fmt --check

# Build WASM (requires wasm-pack)
wasm-pack build crates/tock-core --target web --features core

# Check WASM bundle size
wasm-pack build crates/tock-core --target web --features core --release
gzip -c crates/tock-core/pkg/tock_core_bg.wasm | wc -c
```

## ADRs

Architecture Decision Records live in `docs/adr/`. Read them before making changes to:
- Zero-I/O core (ADR-001)
- End-to-end encryption (ADR-002)
- Event-sourced sync (ADR-003)
- SQLite app-layer encryption (ADR-004)
- Platform bindings (ADR-005)
- Licensing — Apache-2.0 + AGPL-3.0 (ADR-006)
- Monetization — open core (ADR-007)
- Unified domain model (ADR-008)
- Natural language CLI (ADR-009)
- SRP authentication (ADR-010)

## Git Policy

- **Never modify git history.** Do not run any command that creates, modifies,
  or deletes commits, refs, or tags. This includes but is not limited to:
  `git commit`, `git push`, `git rebase`, `git merge`, `git cherry-pick`,
  `git revert`, `git reset`, `git tag`, `git am`, `git stash drop`.
- **Read-only git is fine.** Commands that only inspect state are permitted:
  `git status`, `git diff`, `git log`, `git show`, `git branch --list`,
  `git stash list`, `git rev-parse`, etc.
- **Staging is fine.** `git add` and `git stash push` are permitted for
  preparing diffs or preserving work, since they don't alter commit history.
- Always present proposed changes and let the user decide when to commit.
- This applies to **all** agents, sub-agents, and automated workflows —
  no exceptions, including CI-related or "cleanup" commits.

## CI / Infrastructure Dependency

**Branch protection for this repo is managed via Terraform in `kafkade/github-infra` (`repo_tock.tf`).** The `required_status_checks` list must match the job names in `.github/workflows/ci.yml`. If you rename, add, or remove CI jobs that are used as merge gates, the corresponding IaC config must be updated or PRs will be permanently blocked. Always flag this when proposing workflow changes.

## PR Title Format

Use conventional commits: `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`. For multi-component changes, include the primary component: `feat(crypto): add vault key wrapping`.

## Reference Documents

The full architecture document with all decisions, data model, and platform designs is in `docs/architecture.md`. ADR index is in `docs/adr/README.md`.
