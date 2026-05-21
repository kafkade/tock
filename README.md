# tock

> Unified personal productivity engine — tasks, habits, time tracking,
> and focus timer fused into a single end-to-end encrypted, local-first
> system.

**Status:** Phase 0 — foundation. The repository scaffolding is in place;
nothing is shippable yet. See
[`docs/architecture.md`](docs/architecture.md) for the full design and
[`docs/adr/`](docs/adr/) for accepted decisions.

## Repository layout

```text
tock/
├── Cargo.toml                  # workspace
├── rust-toolchain.toml         # pinned 1.88.0
├── deny.toml                   # cargo-deny config
├── dist-workspace.toml         # cargo-dist config
├── flake.nix                   # Nix dev shell
├── crates/
│   ├── tock-core/              # PURE: zero I/O, zero net, zero async runtime
│   ├── tock-crypto/            # PURE: key hierarchy, AEAD, KDF
│   ├── tock-parse/             # PURE: filter DSL + natural-language parser
│   ├── tock-storage/           # SQLite adapter (rusqlite + sqlcipher)
│   ├── tock-sync/              # event log, conflict res, transport trait
│   ├── tock-import/            # importers (Todoist, Things 3, Toggl, ...)
│   ├── tock-export/            # exporters (JSON, CSV, iCal, hledger)
│   ├── tock-cli/               # `tock` binary (clap + ratatui)
│   ├── tock-server/            # Axum sync server — AGPL-3.0-only
│   └── tock-uniffi/            # UniFFI scaffolding crate (Apple bindings)
├── bindings/
│   └── swift/                  # generated Swift package (Phase 5)
├── apps/
│   ├── ios/                    # SwiftUI iPhone / iPad / watchOS (Phase 5)
│   ├── macos/                  # SwiftUI macOS (Phase 5)
│   └── web/                    # Next.js + WASM (Phase 6)
├── xtask/                      # `cargo xtask` build orchestration
├── docs/
│   ├── architecture.md
│   ├── adr/
│   └── distribution/
└── scripts/
```

## Licensing

Dual-licensed per [ADR-006](docs/adr/ADR-006-licensing-dual-license.md):

- All crates **except** `tock-server` — [Apache-2.0](LICENSE-APACHE).
- `tock-server` — [AGPL-3.0-only](LICENSE-AGPL).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). All commits require a DCO
sign-off (`git commit -s`).

## Quickstart (development)

```sh
# Verify the workspace builds and tests pass
cargo build --workspace
cargo test --workspace

# Lint + format
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# License + advisory audit
cargo install cargo-deny  # one-time
cargo deny check

# WASM smoke build (used by CI)
rustup target add wasm32-unknown-unknown
cargo build -p tock-core --target wasm32-unknown-unknown --no-default-features --features core
```

Nix users: `nix develop` drops you into a shell with the pinned toolchain
and all auxiliary tools.
