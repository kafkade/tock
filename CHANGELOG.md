<!-- markdownlint-disable MD024 -->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Localization (i18n) framework for the CLI: user-facing strings are now translatable via [Fluent](https://projectfluent.org/) (`i18n-embed`). English (`en-US`) ships fully localized in `crates/tock-cli/i18n/`, with a `--lang`/`TOCK_LANG` override and automatic OS-locale detection (falls back to English). Adds `cargo xtask i18n-check` to validate catalog id parity (wired into CI), a translator guide ([`docs/TRANSLATING.md`](docs/TRANSLATING.md)), and a `tr!` macro with compile-time message-id checking for contributors
- Markdown export with Tera templates: `tock export md` with three built-in templates (`--builtin task-list`, `habit-report`, `time-report`), custom template support (`--template <path>`), and optional task filtering (`--filter <expr>`)
- Taskwarrior import: `tock import taskwarrior -f <file>` parses `task export` JSON with field mapping (status, priority, dates, annotations), project creation, dependency linking, recurrence conversion, and UDA registration
- CSV import: `tock import csv -f <file>` with automatic column detection from headers and optional TOML mapping file (`--map config.toml`) for custom column assignments, date formats, and field overrides
- watchOS companion app (`apps/watchos/`): three-tab layout (Today, Habits, Timer) with today's tasks (up to 20, sorted by urgency), tap-to-complete with haptic feedback, habit tracking with tap-to-log and streak display, quick timer and Pomodoro focus sessions with progress ring. WatchConnectivity sync with paired iPhone and persistent intent queue for offline mutations. WidgetKit complications for all accessory families (circular habit ring, rectangular task list/timer, inline status line, corner habit gauge)
- TUI help overlay: press `?` to see all keyboard shortcuts in the detail pane, with arrow key (←/→) pane navigation
- CalDAV bidirectional sync engine (`tock-caldav`): iCalendar parser/serializer (RFC 5545 subset), Task↔VTODO and TimeBlock↔VEVENT mapping, pull→resolve→push sync with ETag conflict retry. CLI commands `tock caldav setup/sync/status/remove` for managing CalDAV collections

### Changed

- Accessibility audit across iOS, macOS, watchOS, and CLI TUI: added VoiceOver labels for icon-only buttons, color-coded indicators, and stateful controls; decorative images hidden from assistive technology; task/time-block rows combined into single accessibility elements; disabled buttons include explanatory hints; theme files documented with WCAG AA contrast notes

## [0.3.0] - 2026-05-30

### Added

- macOS native app (`apps/macos/`): full-window `NavigationSplitView` with three-column layout (sidebar, content, detail), `MenuBarExtra` with compact popover (timer/focus status, today tasks, quick-add, focus controls), and global hotkey (`⌃⌥Space`) floating quick-entry panel via AppKit `NSPanel`. macOS-native `Settings` scene with vault/sync/about tabs. Full command menu structure with keyboard shortcuts per architecture §8.3 (`⌘N` new task, `⌘1`–`⌘7` view switching, `Space` complete, `⌘E` evening, `⌘T` timer, `⌘⇧F` focus, `⌘⌥L` lock vault). Uses `@SceneStorage` for per-window state restoration and shared `AppSessionState` across scenes. Carbon `RegisterEventHotKey` for reliable system-wide hotkey. Uses `CoreClient` protocol with mock data for development until UniFFI bindings are connected
- CI path-based job skipping: Rust, CI/infra, and docs change detection via `dorny/paths-filter` to skip irrelevant jobs on PRs
- CI WASM bundle-size gate: enforces 2 MB compressed budget on `tock-core` WASM builds
- CI MSRV job: verifies the workspace compiles on the minimum supported Rust version (1.95)
- CI markdown lint job: runs `markdownlint-cli2` on all markdown files
- Scheduled weekly security audit workflow (`cargo audit`) with automatic issue creation on failure
- Release version/tag validation: verifies git tag matches `Cargo.toml` workspace version before publishing
- Release crates.io publishing: manual workflow with dependency-ordered dry-run, publish, and index-wait per crate
- SwiftUI iOS app (`apps/ios/`): five-tab navigation (Today, Inbox, Projects, Habits, Timer), vault unlock gate, quick-add sheet, task/habit/project detail views, focus session UI with Pomodoro cycle tracking, and settings view. Uses `CoreClient` protocol with mock data for development until UniFFI bindings are connected
- iPadOS adaptive layout: three-column `NavigationSplitView` (sidebar, content, detail) on iPad with automatic fallback to tab bar on iPhone. Sidebar shows smart views, projects, areas, habits, and timer
- iPad keyboard shortcuts: `⌘N` (new task), `⌘1`–`⌘5` (switch view), `⌘6`/`⌘7` (habits/timer), `Space` (mark done), `⌘E` (evening), `⌘T` (timer), `⌘⇧F` (focus session). Routed per-window via `FocusedValues`
- iPad drag-and-drop: drag tasks onto sidebar projects to reassign. Uses ID-only `Transferable` wrapper to avoid leaking plaintext task data via pasteboard
- Stage Manager multi-window support: each window owns independent `AppState` for sidebar selection and task detail
- Biometric vault unlock: Face ID and Touch ID support via `LAContext` and iOS Keychain. Vault key cached with `.biometryCurrentSet` access control (auto-invalidated on biometric enrollment changes). Includes reinstall detection, auto-trigger on lock screen, explicit error messaging, and enable/disable toggle in Settings
- WidgetKit widgets in seven size families: small (timer/next task), medium (today list with interactive checkboxes), large (tasks + habit strip + timer), extra-large iPadOS two-column (today + inbox), and lock screen accessories (habit ring, next task, status line). Interactive buttons use App Intents for completing tasks and logging habits. Deep-links into the app on tap. Shows lock icon when vault is locked
- App Intents for Siri and Shortcuts: 11 intents covering task capture, completion, timer control, focus sessions, habit logging, streak queries, navigation, and reports. Four AppEntity types (Task, Habit, Project, Report) with string-based search for voice resolution. Six pre-built shortcuts installable from the Shortcuts app. Deep-link URL handling for navigation intents
- Share extension for quick capture from Safari, Mail, Notes, and other apps. Extracts URLs with page title metadata, plain text, and file/image references. Editable capture form with project picker, tags, priority, and destination chooser (Inbox/Today/Evening/Someday). Pending captures stored as JSON and drained on next app launch

## [0.2.1] - 2026-05-30

### Changed

- Update CI pinned rust version

## [0.2.0] - 2026-05-30

### Added

- Event log wire format: binary serialization/deserialization for `SignedEvent` batches with encrypted batch envelope for E2EE transport
- Sync transport trait and types: `SyncCursor`, `PushAck`, `PullBatch`, `Transport` trait for pluggable sync backends
- Conflict resolution engine: vector-clock-based detection (supersedes/stale/duplicate/concurrent), per-field merge for disjoint updates, last-writer-wins for overlapping fields, configurable delete-vs-update policy, conflict log for user review
- Stateless sync engine: `process_incoming_event` with transactional head classification and merged device clock
- Device pairing flow: X25519 key exchange, SHA-256 fingerprint verification, onboarding blob with AAD-bound VK encryption, 5-minute invite expiry
- Device revocation: append-only status changes preserving verifying keys for historical signature verification
- Recovery key: Crockford Base32 encoding (52 chars), HKDF-derived key, VK wrap/unwrap via recovery path
- Password rotation: re-derives MK/MEK from new password, re-wraps VK without changing item keys
- Vault key rotation planning: generates new VK and lists entities requiring re-encryption (plan-only, no mutation)
- Self-hosted sync server (`tock-server`): Axum HTTP API with push/pull events, device registration, onboarding blob storage, and health check endpoint. Server stores only opaque encrypted blobs and never decrypts user data (AGPL-3.0)
- Dockerfile: multi-stage build producing a minimal Debian-based container image for tock-server
- Docker Compose configuration for simple self-hosting with persistent storage
- Helm chart for Kubernetes deployment with resource limits, health/readiness probes, and persistent volume claim
- systemd unit file for traditional server deployments with security hardening
- Hosted sync service skeleton: `--mode hosted` enables user accounts, subscription tiers (Free/Personal/Family/Pro), per-account rate limiting, usage tracking (encrypted byte counts only), and a `/metrics` endpoint
- User-defined attributes (UDAs): `tock uda add effort --type number` to declare custom fields on tasks. Set values with `tock mod <sid> uda.effort:5`. Filter with `uda.effort:5` in list/view commands
- Urgency scoring engine with configurable coefficients and `tock urgency <sid>` breakdown. Tasks auto-sorted by urgency in list views
- Hook scripts API: external scripts at `~/.config/tock/hooks/` for lifecycle events (`on-add`, `on-complete`). Pre-hooks can modify or cancel operations. `tock hooks ls/path`
- Custom report definitions: `tock report define/show/ls/rm` with saved filters, sorting, and column selection
- Task dependencies: `tock depend/undepend`, dependency-aware blocked/blocking filters (`+BLOCKED`, `+BLOCKING`), blocked urgency penalties, and dependency details in `tock show`
- Recurring tasks: `tock add --recur daily|weekly|monthly|yearly|every-3d|every-2w`, automatic next-instance creation on completion, and recurrence details in `tock show`
- Named contexts: `tock context define/set/clear/list/rm` for reusable filters, with active contexts automatically applied to `tock ls`, `tock view`, and `tock report show`
- Pomodoro focus timer: `tock focus start/done/skip-break/pause/resume/stop/status/stats` with configurable intervals and automatic time-block logging
- Focus session history per task: `tock focus history <sid>` and auto-stop on `tock done`
- Time block editing: `tock time edit <sid>` with `--title/--start/--end/--task/--billable` flags
- Habit tracking: CRUD, identity statements, stacking, cadences (daily/weekly/specific-days), Fibonacci leveling, streak tracking with skip/freeze grace days, backfill logging, break-bad-habit mode, and per-habit reminders
- UniFFI bindings (`tock-uniffi`): full FFI facade exposing all four domains (tasks, habits, time tracking, focus sessions) to Apple platforms via a `Workspace` object with 30+ methods, UniFFI proc-macro types, and a `uniffi-bindgen` CLI for Swift code generation
- Swift Package Manager layout (`bindings/swift/`): `TockFFI` target for generated bindings and `TockSwift` target with idiomatic async/await wrapper, targeting iOS 17+ and macOS 14+
- Cross-platform notifications on focus events (stderr-based, upgradeable to desktop)
- Interactive ratatui TUI launched with `tock tui`, with sidebar views/projects, task list, detail pane, vim-style navigation, refresh, complete, and delete actions
- README rewritten with full feature documentation and install instructions

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
