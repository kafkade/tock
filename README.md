# tock

> Unified personal productivity engine — tasks, habits, time tracking,
> and focus timer fused into a single end-to-end encrypted, local-first
> system.

## Features

### Task management

```sh
tock add "Buy groceries #errands !H due:tomorrow"
tock ls                          # list tasks sorted by urgency
tock show 1                      # task detail view
tock mod 1 !M due:friday         # modify priority + deadline
tock done 1 2 3                  # batch complete
tock view today                  # built-in smart views
tock urgency 1                   # explain urgency breakdown
```

Sigil syntax for inline metadata: `#tag`, `!H/M/L` (priority),
`due:tomorrow` (natural language dates). Filter with `status:pending`,
`tag:work`, `+TODAY`, `+OVERDUE`. Six built-in views: inbox, today,
upcoming, anytime, someday, logbook.

### Time tracking

```sh
tock time start "Review PR #code"   # auto-creates task + starts timer
tock time start 1                   # start on existing task
tock time stop                      # stop current block
tock time resume                    # re-start most recent
tock time blocks today              # list blocks
tock time report week               # aggregated report
tock time edit 1 --title "Updated"  # edit a block
```

### Focus timer (Pomodoro)

```sh
tock focus start --task 1 --cycles 4 --work 25
tock focus done              # complete a cycle → auto-logs time block
tock focus skip-break        # skip break, start working
tock focus pause / resume / stop
tock focus status            # show active session
tock focus stats today       # completed cycles, focus time
tock focus history 1         # per-task focus history
```

### Habit tracking

```sh
tock habit add "Read 10 pages" --identity "I am a reader" --cue "After dinner" --daily
tock habit add "No social media" --direction break
tock habit log 1                 # log completion
tock habit slip 2                # log a break-habit slip
tock habit skip 1 --reason flu   # skip without breaking streak
tock habit status                # all habits with streaks + levels
tock habit streaks 1             # streak history
tock habit remind 1 --at 07:00   # set reminder
```

Identity statements, habit stacking (`--stack-after`), cadences (daily,
N×/week, specific days), Fibonacci leveling (Spark → Embodied), and
grace days (skip/freeze).

### Task dependencies

```sh
tock depend 1 on 2               # task 1 blocked until task 2 is done
tock undepend 1 from 2           # remove dependency
tock ls +BLOCKED                 # list blocked tasks
tock ls +BLOCKING                # list tasks blocking others
tock show 1                      # shows dependencies + dependents
```

Circular dependency detection walks the chain (max depth 100). Blocked
tasks receive an urgency penalty so they sink in listings.

### Recurring tasks

```sh
tock add "Pay rent" --recur monthly
tock add "Standup" --recur daily
tock add "Review" --recur every-2w
tock done 1                      # auto-creates next instance
tock show 1                      # shows recurrence details
```

Supports `daily`, `weekly`, `monthly`, `yearly`, `every-Nd`, `every-Nw`.
Periodic mode anchors next instance to the due date; chained mode anchors
to the completion date.

### Named contexts

```sh
tock context define work "tag:work status:pending"
tock context set work            # all listings now filter by work
tock ls                          # shows [ctx: work] prefix
tock context clear               # remove active context
tock context ls                  # list all contexts (* = active)
tock context rm work
```

Contexts are named saved filters that are automatically AND-ed into
every `tock ls`, `tock view`, and `tock report show` command.

### Interactive TUI

```sh
tock tui
```

Three-pane ratatui terminal interface: sidebar (views + projects), task
list (sorted by urgency), and task detail. Vim-style keys: `j`/`k` to
navigate, `Tab` to switch panes, `Enter` to select, `d` to complete,
`x` to delete, `r` to refresh, `q` to quit.

### More

- **Projects & areas** — `tock project add/ls/archive`, `tock area add/ls`
- **Tags** — `#tag` sigils, `tock tag ls/rename`
- **Urgency scoring** — `tock urgency 1` for component breakdown
- **Custom reports** — `tock report define overdue --query '+OVERDUE' --sort deadline`
- **User-defined attributes** — `tock uda add effort --type number`, filter with `uda.effort:5`
- **Hook scripts** — external scripts at `~/.config/tock/hooks/` for lifecycle events
- **Output formats** — `--format table|compact|json`, per-command `--json`
- **Shell completions** — `tock completions bash|zsh|fish|elvish|powershell`
- **JSON import/export** — `tock export json`, `tock import json --file backup.json`
- **Encrypted vault** — password-protected, AES-256-GCM per-event AEAD, Ed25519 signed event log
- **Multi-device sync** — end-to-end encrypted sync through a self-hostable server; see the [dogfooding guide](docs/dogfooding.md)
- **Self-hosting** — one-command Docker Compose stack with a web admin console, first-run wizard, and automatic TLS; see the [self-hosting guide](docs/self-hosting.md)

## Install

Download the latest release from
[GitHub Releases](https://github.com/kafkade/tock/releases), or build
from source:

```sh
git clone https://github.com/kafkade/tock.git
cd tock
cargo build --release -p tock-cli
# Binary at target/release/tock (or tock.exe on Windows)
```

## Repository layout

```text
tock/
├── Cargo.toml                  # workspace
├── rust-toolchain.toml         # pinned 1.88.0
├── deny.toml                   # cargo-deny config
├── dist-workspace.toml         # cargo-dist config
├── flake.nix                   # Nix dev shell
├── crates/
│   ├── tock-core/              # PURE: domain model, urgency, crypto types
│   ├── tock-crypto/            # PURE: AES-256-GCM, Argon2id, HKDF, Ed25519
│   ├── tock-parse/             # PURE: filter DSL + natural-language dates
│   ├── tock-storage/           # SQLite vault, repos, migrations
│   ├── tock-cli/               # `tock` binary (clap CLI + ratatui TUI)
│   ├── tock-import/            # JSON importer
│   ├── tock-export/            # JSON exporter
│   ├── tock-sync/              # event log, sync protocol (foundation)
│   ├── tock-server/            # Axum sync server — AGPL-3.0-only
│   └── tock-uniffi/            # UniFFI scaffolding (Apple bindings)
├── docs/
│   ├── architecture.md
│   ├── dogfooding.md           # self-host + multi-device sync guide
│   ├── adr/                    # 10 Architecture Decision Records
│   └── distribution/
└── scripts/
    └── release.ps1             # automated release script
```

## Licensing

Dual-licensed per [ADR-006](docs/adr/ADR-006-licensing-dual-license.md):

- All crates **except** `tock-server` — [Apache-2.0](LICENSE-APACHE).
- `tock-server` — [AGPL-3.0-only](LICENSE-AGPL).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). All commits require a DCO
sign-off (`git commit -s`).

## Development

```sh
# Build + test
cargo build --workspace
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check

# WASM smoke build (CI gate)
cargo build -p tock-core --target wasm32-unknown-unknown --no-default-features --features core
```

Nix users: `nix develop` drops you into a shell with the pinned toolchain
and all auxiliary tools.
