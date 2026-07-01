<!-- markdownlint-disable MD024 -->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Account signup & login with Secret Key onboarding across all clients** (#129): a new zero-I/O `tock-account` crate orchestrates the full account lifecycle — generate a Secret Key, derive the SRP verifier (2SKD), register, and sign in on a fresh device with only email + password + Secret Key. Signup emits an **Emergency Kit** (printable text + PDF) and a **Setup Code** (`TOCK1:` text + scannable QR). The CLI gains `tock account signup/login/logout/status`, storing session credentials in the OS keyring (file fallback for headless) and authenticating every sync request with `Authorization: Bearer` + `X-Tock-Channel-Binding`. Apple apps gain a UniFFI account API (`account_signup_bundle`, SRP login state machine, Setup-Code parsing), Keychain-backed token + channel-binding storage, and channel-bound authed sync. The server stores the wrapped vault header so a new device can recover its keys after SRP login (`PUT`/`GET /v1/vaults/:id/header`), and `srp/start` now returns `kdf_params` so a fresh device derives its Unlock Root Key before authenticating. The password is never persisted or transmitted
- **Web app** (`apps/web`): a new React + TypeScript + Vite app drives signup, login, and authed sync entirely in the browser via a new `tock-wasm` (wasm-bindgen) binding that exposes the `tock-account` orchestration; signup shows the Emergency Kit + Setup Code (with QR), credentials default to in-memory (sessionStorage opt-in) and the Secret Key is never persisted in the browser. The WASM bundle stays well under the 2 MB gzip budget; a `web` CI job builds the package, enforces the size gate, and runs the Vitest suite
- **One-command self-hosting** (#132): a polished, Immich-style "run it yourself" path. `docker compose up -d` now brings up an `.env`-driven stack — `tock-server` behind a new `tock-web` **admin console** (separate Apache-2.0 nginx container, `Dockerfile.web`) that serves the SPA and proxies the API on a single HTTP entrypoint. A **first-run wizard** creates the admin account and forces saving the Emergency Kit; the console adds user management (invite/enable/disable/delete) and a registration-policy control, backed by a new public `GET /v1/server/info` endpoint for setup gating and an optional `TOCK_ADMIN_USERNAME` env bootstrap. Optional **TLS** via a bundled Caddy profile (auto Let's-Encrypt), with nginx and Traefik samples in `deploy/proxy/`. `deploy/helm` and `deploy/systemd` gain the account/auth env vars. A new [self-hosting guide](docs/self-hosting.md) covers compose + `.env`, the first-run flow, TLS, connecting the CLI and Apple apps, user management, and backup/restore/upgrades/health

## [0.4.0] - 2026-06-28

### Added

- **SRP login + account-scoped, authenticated sync** for `tock-server` (#130): an SRP-6a login handshake (`POST /v1/auth/srp/start` → `B` + salt, `POST /v1/auth/srp/finish` → `M2` + a short-lived bearer session) authenticates a client without the server ever seeing the password, Secret Key, or session key `K`. Both sides derive the bearer token and a channel-binding tag from `K` via HKDF (the token is never sent in the handshake bodies); the server stores only a SHA-256 hash of the token. All sync routes (`devices`, `events/push`, `events/pull`, onboarding) now require a valid session — unauthenticated requests are rejected with `401`. Vaults are bound to the account that first claims them, and a session may only touch its own account's vaults (`403` otherwise). The channel-binding tag is verified on the event routes as defense-in-depth. `POST /v1/auth/refresh` slides the session TTL forward. Admin endpoints now accept an SRP session token from an admin account (the interim admin api-token still works). Payloads remain ciphertext-only — the server still cannot decrypt user data
- Self-hosted **account system** for `tock-server` (admin/user management, Immich-style): the server now stores first-class accounts carrying an SRP verifier (`srp_salt`, `srp_verifier`, `srp_group`, `kdf_params`) — never a plaintext password, Secret Key, or 2SKD root, so the server stays zero-knowledge. The first registration on a fresh instance is bootstrapped as an `admin` (first-run guard against hijack); thereafter a configurable **registration policy** (`open` / `invite-only` / `disabled`, default `disabled`) governs who may register, set via the `TOCK_REGISTRATION_POLICY` env var or the admin API. New admin endpoints `POST /v1/accounts/register` and `/v1/admin/users…` + `/v1/admin/settings` let an admin invite, disable, enable, and delete users and change the policy (admins mint single-use invite tokens rather than setting passwords). A `tock-server admin` CLI (`create-admin`, `list-users`, `reset-registration`) provides offline administration. Registration is rate-limited per client IP by the existing limiter. Hosted billing (`--mode hosted`) is unchanged. SRP login handshake, session tokens, and vault binding land in #130
- Account **Secret Key** and two-secret key derivation (2SKD), per ADR-011: the vault's Unlock Root Key is now derived from **both** your password and a 128-bit, client-generated Secret Key (`URK = Argon2id(password) XOR HKDF(secret_key)`), so a stolen vault file or server verifier is uncrackable without the Secret Key. The Secret Key is generated locally on `tock init` and printed once as an Emergency Kit string (`A4-…`); it is never written to the vault file or sent to any server. Opening a vault now requires the Secret Key via `--secret-key` or the `TOCK_SECRET_KEY` env var. Device pairing (`tock onboard accept`, Apple "Join Existing Vault") prompts for the account Secret Key so the new device joins the same account. Crockford Base32 now lives in `tock-crypto::base32` as the canonical codec
- iOS, macOS, and watchOS apps now run on the real Rust core instead of mock data (#121): each app opens a real encrypted vault via `TockSwift.TockWorkspace` (UniFFI). The iOS/macOS vault lives in the App Group container (`group.com.kafkade.tock`) so the app, widgets, App Intents, and the Share Extension share one CLI-readable vault; iOS biometric unlock caches the master password in the Keychain. The watch runs as a read-replica: the new iPhone-side `PhoneSessionManager` pushes a snapshot of today's tasks/habits/timer/focus over WatchConnectivity and applies watch-originated mutation intents back to the vault. `MockCoreClient`/`MockWatchCoreClient` are retained only for SwiftUI previews and tests; a `LockedCoreClient` sentinel ensures no production path falls back to mock data while locked
- Apple apps can configure a sync server, pair a new device with QR or manual codes, and push or pull encrypted changes from Settings with an optional hosted auth token
- Multi-device sync over HTTP: `tock sync` pushes local changes and pulls remote ones against a self-hosted `tock-server`, runs the conflict-resolution engine, and prints a `pushed N, pulled M, conflicts K` summary. `--server <url>` configures and persists the server on first use; `--dry-run` reports pending local changes without contacting the server
- Sync conflict review: `tock sync conflicts` lists unresolved concurrent-edit conflicts and `tock sync resolve <id>` acknowledges one — no silent last-write-wins for productivity data (ADR-003)
- Device pairing: `tock onboard invite` (existing device) and `tock onboard accept` (new device) transfer the vault key over an end-to-end-encrypted X25519 channel with an out-of-band fingerprint check, then create and back-fill a local vault for the new device
- Device management: `tock device ls` lists registered devices with status and `tock device revoke <id>` revokes one; the revocation propagates to peers on the next sync
- `HttpTransport` (in `tock-cli`, per ADR-001): a concrete `tock_sync::Transport` implementation over the server's `push`/`pull`/`devices`/`onboarding` REST endpoints; the server only ever stores ciphertext
- Event-sourcing sync substrate (`tock-storage::sync`): synthesizes signed, AEAD-encrypted events by diffing domain state against a journal at sync time, covering tasks, projects, areas, headings, tags, time blocks, focus sessions, habits, and devices through one code path
- Localization (i18n) framework for the CLI: user-facing strings are now translatable via [Fluent](https://projectfluent.org/) (`i18n-embed`). English (`en-US`) ships fully localized in `crates/tock-cli/i18n/`, with a `--lang`/`TOCK_LANG` override and automatic OS-locale detection (falls back to English). Adds `cargo xtask i18n-check` to validate catalog id parity (wired into CI), a translator guide ([`docs/TRANSLATING.md`](docs/TRANSLATING.md)), and a `tr!` macro with compile-time message-id checking for contributors
- UniFFI Swift bindings now link against the real Rust core: `import TockSwift` exposes a working `TockWorkspace` with idiomatic async/await wrappers across all domains (tasks, projects, areas, tags, time tracking, focus sessions, habits). Bindings and the `TockFFI.xcframework` (macOS + iOS device + iOS simulator slices) are regenerated reproducibly via `cargo xtask xcframework`
- Markdown export with Tera templates: `tock export md` with three built-in templates (`--builtin task-list`, `habit-report`, `time-report`), custom template support (`--template <path>`), and optional task filtering (`--filter <expr>`)
- Taskwarrior import: `tock import taskwarrior -f <file>` parses `task export` JSON with field mapping (status, priority, dates, annotations), project creation, dependency linking, recurrence conversion, and UDA registration
- CSV import: `tock import csv -f <file>` with automatic column detection from headers and optional TOML mapping file (`--map config.toml`) for custom column assignments, date formats, and field overrides
- watchOS companion app (`apps/watchos/`): three-tab layout (Today, Habits, Timer) with today's tasks (up to 20, sorted by urgency), tap-to-complete with haptic feedback, habit tracking with tap-to-log and streak display, quick timer and Pomodoro focus sessions with progress ring. WatchConnectivity sync with paired iPhone and persistent intent queue for offline mutations. WidgetKit complications for all accessory families (circular habit ring, rectangular task list/timer, inline status line, corner habit gauge)
- TUI help overlay: press `?` to see all keyboard shortcuts in the detail pane, with arrow key (←/→) pane navigation
- CalDAV bidirectional sync engine (`tock-caldav`): iCalendar parser/serializer (RFC 5545 subset), Task↔VTODO and TimeBlock↔VEVENT mapping, pull→resolve→push sync with ETag conflict retry. CLI commands `tock caldav setup/sync/status/remove` for managing CalDAV collections

### Changed

- **Breaking (vault format v2):** the encrypted vault header now records an `account_id` and `kdf_version` and roots key derivation in the 2SKD Unlock Root Key. Pre-1.0 password-only (v1) vaults are not auto-migrated: they are detected and rejected with a clear "re-initialize required" error. Re-run `tock init` to create a fresh v2 vault and Secret Key, then re-import your data
- `tock_sync::Transport` is now an `async` trait (`async-trait`), enabling real network transports; the CLI drives it via a Tokio runtime for sync commands. `tock-sync` itself stays runtime-free and WASM-safe
- Sync server pull cursor is now a monotonic server-assigned position (row order) instead of a per-device Lamport value, so an offline device whose Lamport lags can no longer miss events on pull
- Allowed the `CDLA-Permissive-2.0` license in `deny.toml` for the Mozilla root-certificate data in `webpki-roots`, pulled in transitively by `reqwest` for the CLI's HTTP sync transport
- Accessibility audit across iOS, macOS, watchOS, and CLI TUI: added VoiceOver labels for icon-only buttons, color-coded indicators, and stateful controls; decorative images hidden from assistive technology; task/time-block rows combined into single accessibility elements; disabled buttons include explanatory hints; theme files documented with WCAG AA contrast notes

### Fixed

- Lifecycle operations (`tock add`, `modify`, `done`, and other hooked commands) are no longer silently cancelled when no matching hook script is installed; cancellation is now reserved for an installed hook that explicitly exits non-zero

### Security

- Self-hosted **and** hosted `tock-server` sync, device registration, and onboarding routes now require an authenticated session before a vault can be claimed or synchronized (#130): self-hosted instances authenticate via SRP login session tokens, hosted instances via their bearer token. Unauthenticated requests are rejected with `401`. Vaults are bound to the account that first claims them and a session may only touch its own account's vaults (`403` otherwise); the SRP channel-binding tag is verified on the event routes as defense-in-depth. Payloads remain ciphertext-only — the server still cannot decrypt user data
- Zero-knowledge account login with SRP-6a (RFC 5054, 4096-bit group, SHA-256), per ADR-010: the server authenticates you without ever receiving your password, account Secret Key, or the derived Unlock Root Key. Registration sends only a verifier folded over **both** secrets, so a stolen verifier cannot be brute-forced without the Secret Key, and login proves knowledge of both secrets with mutual client/server proofs — a wrong password **or** a wrong Secret Key is rejected. The resulting session key is bound into a short-lived sync bearer token and an event channel-binding tag

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
