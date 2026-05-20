# ADR-001: Rust with zero-I/O core crate

**Status:** Accepted  
**Date:** 2026-05-20

## Context

Tock needs to run consistently across CLI, iOS, watchOS, macOS, and web (WASM) platforms. Traditional approaches tightly couple business logic to platform-specific I/O APIs (filesystem, networking, async runtimes), requiring separate implementations per target. This leads to divergence, inconsistent behavior, and increased testing surface.

We need a single source of truth for task management, habit tracking, time tracking, and focus timer logic that compiles to native binaries and WASM without modification, while remaining testable in isolation.

## Decision

The core crate (`tock-core`) is **pure computation**: zero filesystem access, zero networking, zero async runtime dependencies. All I/O is injected via traits implemented by platform-specific crates:

- `tock-storage` provides SQLite storage for native targets.
- `tock-storage-web` provides IndexedDB storage for WASM.
- `tock-sync` defines transport traits implemented by HTTP clients or peer-to-peer layers.

Core logic handles all domain rules—urgency scoring, recurrence expansion, conflict resolution, encryption key derivation—as pure functions or state machines. The CLI calls core directly; UniFFI generates Swift bindings for Apple platforms; wasm-bindgen exposes it to JavaScript.

This enforces a strict contract verified by CI: `cargo tree -p tock-core --edges normal` must not list `tokio`, `reqwest`, `rusqlite`, `std::fs`, or `std::net`. An `xtask check-purity` job scans dependency manifests and fails the build on violation.

## Consequences

**Positive:**
- Identical behavior across all platforms (same Rust code executes everywhere).
- Core is provably WASM-compatible and deterministic (no hidden I/O side effects).
- Fast, exhaustive testing without mocking filesystem or network.
- Platform code handles only presentation and bridging to system APIs.

**Negative:**
- Requires discipline: new features must never import I/O crates into core.
- Trait-based injection adds indirection (minimal runtime cost, slight complexity).
- Contributors unfamiliar with dependency inversion may find the boundary counterintuitive.

**Neutral:**
- Platform bindings (UniFFI, wasm-bindgen) own async surfaces; core remains synchronous.
