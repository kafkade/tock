# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-21

### Added

- Repository scaffolding: GitHub templates, CI/release workflows, copilot instructions, contribution guide, and licensing (Apache-2.0 for client code, AGPL-3.0 for sync server)
- Architecture design document (`docs/architecture.md`) and Architecture Decision Records (`docs/adr/ADR-001` through `ADR-010`)
- Cargo workspace scaffold per `docs/architecture.md` §4.1: `tock-core`, `tock-crypto`, `tock-parse`, `tock-storage`, `tock-sync`, `tock-import`, `tock-export`, `tock-cli`, `tock-server`, `tock-uniffi`, plus `xtask`. Every crate is a minimal compilable placeholder
- Workspace lint table enforcing `unsafe_code = forbid`, `missing_docs`, clippy pedantic/nursery, and `deny` on `unwrap`/`expect`/`panic`/`todo` (`tock-uniffi` opts out of `unsafe_code` for FFI generation per ADR-005)
- `rust-toolchain.toml` pinning Rust 1.88.0 (edition 2024) with `rustfmt`, `clippy`, and the `wasm32-unknown-unknown` target
- `deny.toml` cargo-deny configuration (license allow-list, advisory and bans gates, registry-only sources)
- `.cargo/config.toml` with `cargo xtask` alias
- `dist-workspace.toml` for `cargo dist` (validated in CI; release workflow migration deferred)
- `flake.nix` Nix dev shell wired to the pinned toolchain plus `cargo-deny`, `cargo-llvm-cov`, `wasm-pack`
- `docs/distribution/` documenting release channels, including a Homebrew formula template at `docs/distribution/homebrew/tock.rb`
- CI pipeline expanded with `cargo deny`, `cargo dist plan`, and code coverage (Linux-only via `cargo-llvm-cov` + Codecov; non-gating initially)
- CI pinned to Rust 1.88.0 in every job to match `rust-toolchain.toml`
- Cryptographic primitives in `tock-crypto`: AES-256-GCM authenticated encryption, Argon2id password hashing with validated `Argon2Params::TOCK_V1` matching the vault format, HKDF-SHA256 key derivation (with 32-byte convenience), X25519 Diffie-Hellman with rejection of contributory (all-zero) shared secrets, Ed25519 sign/verify with strict verification
- `SecretBytes<N>` wrapper providing zeroize-on-drop, constant-time equality, and a redacted `Debug` impl; `Zeroizing<Vec<u8>>` returned from AEAD decrypt so plaintext is wiped on drop
- All RNG-touching constructors in `tock-crypto` (`try_random`, `try_generate`) return `Result` so callers can handle OS RNG failure without panicking
- Vault format and key hierarchy: `tock vault init/open/lock/status` operations with a SQLite-backed on-disk format. Password → MK (Argon2id) → MEK (HKDF) → wraps Vault Key (AES-256-GCM, header bound as AAD so tampering invalidates the wrap). VK derives per-entity-kind domain keys and per-item keys for the event log
- Append-only event log signed with Ed25519 and per-entity AEAD-encrypted payloads. Events are written and read through a single `EventLog` API; signatures must match the device registry, and plaintext payloads never touch disk
- Embedded SQL migration framework: numbered migrations are applied in a transaction with SHA-256 checksums tracked in `schema_migrations`; checksum mismatches refuse to open the vault (developer/schema integrity check)
- Device registry: each vault registers its local device's Ed25519 verifying key under a random 16-byte device id; event verification rejects events signed by unregistered devices
- Vault open/init returns `InvalidVaultOrCredentials` for both wrong passwords and tampered headers so the cause is indistinguishable to a caller; missing-file remains distinct
- `tracing`-based structured logging with vault-data redaction: span instrumentation on vault init/open and event append; deny-list of sensitive field names; human-readable and JSON output formats selectable via `TOCK_LOG_FORMAT` environment variable
- Task management CLI: `tock add`, `tock mod`, `tock done`, `tock cancel`, `tock delete`, `tock ls`, `tock show` commands with sigil syntax for tags (`#tag`), priority (`!H/M/L`), and deadline (`due:YYYY-MM-DD`). Human-readable table and JSON output formats
- Project and area management: `tock project add/ls/archive`, `tock area add/ls` with per-project headings
- Flat tag system with `#tag` sigil syntax: `tock tag ls`, `tock tag rename`. Tags are automatically created on first use and applied via the N:N `entity_tags` join table
- Domain types for tasks, projects, areas, headings, and tags in `tock-core` with SID (short ID) allocation per entity kind
- SQLite repository layer in `tock-storage` with typed CRUD: `task_repo`, `project_repo`, `area_repo`, `heading_repo`, `tag_repo`, `sid_repo`
- Natural language date parser: `tomorrow`, `next friday`, `in 3 days`, `eow` (end of week), `eom` (end of month), ISO dates (`YYYY-MM-DD`), and weekday names. Used automatically when setting deadlines via `due:tomorrow`
- Filter language with `status:X`, `tag:X`, `priority:X`, `project:X`, virtual tags `+TODAY`, `+OVERDUE`, `+EVENING`, logical `NOT`, and implicit `AND` for multiple filter terms
- Six built-in views: `tock view inbox`, `tock view today`, `tock view upcoming`, `tock view anytime`, `tock view someday`, `tock view logbook`. List available views with `tock views`
- Output formatters: `--format table` (default), `--format compact` (one-liner per task), `--format json`. Per-command `--json` shorthand
- Shell completion generation: `tock completions bash|zsh|fish|elvish|powershell` prints completions to stdout
- JSON import/export for testing and backup: `tock export json` (to stdout or `--out file.json`) and `tock import json --file tasks.json`
- Time tracking: `tock time start/stop/resume/current` commands with automatic task creation on `start` when given a description instead of a task SID
- Time block listing: `tock time blocks today|week|month|all` with table and JSON output
- Time reports: `tock time report today|week|month` with per-title aggregation and totals
