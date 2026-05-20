# Contributing to tock

Thank you for your interest in contributing to tock! This document covers how to
build the project, our development workflow, and contribution requirements.

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). Please be respectful and constructive in all interactions.

## Prerequisites

- **Rust 1.85+** — install via [rustup](https://rustup.rs/)
- **wasm-pack** (optional) — for WASM builds: `cargo install wasm-pack`

## Building from Source

```sh
# Clone the repository
git clone https://github.com/kafkade/tock.git
cd tock

# Build all crates
cargo build --workspace

# Build the CLI
cargo build -p tock-cli

# Build WASM (requires wasm-pack)
wasm-pack build crates/tock-core --target web --features core
```

## Running Tests

```sh
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p tock-core

# Run a single test by name
cargo test -p tock-core test_name

# Run tests for a specific module
cargo test -p tock-core tasks::
```

## Code Quality

All code must pass these checks before merging:

```sh
# Formatting (rustfmt)
cargo fmt --check
# Fix formatting issues:
cargo fmt

# Linting (clippy)
cargo clippy --workspace --all-targets -- -D warnings

# All checks run in CI on every pull request
```

## Development Workflow

1. **Fork and clone** the repository.
2. **Create a feature branch** from `main`:

   ```sh
   git checkout -b feat/my-feature
   ```

3. **Make your changes** and ensure all checks pass.
4. **Sign off your commits** (DCO requirement — see below):

   ```sh
   git commit -s -m "feat: add my feature"
   ```

5. **Open a pull request** against `main`.

### Commit Messages

We follow [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` — new feature
- `fix:` — bug fix
- `docs:` — documentation changes
- `test:` — adding or updating tests
- `refactor:` — code restructuring without behavior change
- `chore:` — maintenance tasks (CI, dependencies, etc.)

For multi-component changes, include the component:
`feat(core): add unified query language parser`

### Pull Request Checklist

- [ ] Tests pass (`cargo test --workspace`)
- [ ] Clippy passes (`cargo clippy --workspace -- -D warnings`)
- [ ] Formatting passes (`cargo fmt --check`)
- [ ] Commits are signed off (DCO)
- [ ] PR description follows the template

## Architecture Guidelines

### tock-core Must Have Zero I/O

The core library (`crates/tock-core/`) must not depend on networking, file system
access, or platform-specific APIs. All I/O happens in platform-specific code
(CLI, iOS, web). This keeps the core testable and compilable to WASM.

### Four Unified Domains

Tasks, habits, time tracking, and focus sessions share IDs, events, and
cross-domain primitives. Changes to the domain model must consider all four
domains — never introduce domain-specific silos.

### Encryption

- All encryption uses audited [RustCrypto](https://github.com/RustCrypto) crates.
- No custom cryptographic primitives.
- Key material must implement `Zeroize` and `ZeroizeOnDrop`.
- `Debug` implementations must redact secret values.

### Error Handling

- Use `thiserror` for library errors in core crates.
- Use `anyhow` only in binary crates (`tock-cli`, `tock-server`).
- Crypto failures must never expose key material in error messages.

## Developer Certificate of Origin (DCO)

All contributions to this project must be signed off under the
[Developer Certificate of Origin](DCO) (DCO). By signing off your commits, you
certify that you wrote the code or have the right to submit it under the
project's license.

Add the sign-off to your commits with `git commit -s` or manually:

```text
Signed-off-by: Your Name <your.email@example.com>
```

This is a lightweight alternative to a CLA (Contributor License Agreement),
used by projects like the Linux kernel and many CNCF projects.

## License

- All code except the sync server is licensed under [Apache-2.0](LICENSE-APACHE).
- The sync server (`crates/tock-server/`) is licensed under [AGPL-3.0](LICENSE-AGPL).

By contributing, you agree that your contributions will be licensed under the
same license as the component you are contributing to.
