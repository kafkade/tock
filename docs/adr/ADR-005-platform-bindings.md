# ADR-005: Platform bindings via UniFFI and WASM

**Status:** Accepted  
**Date:** 2026-05-20

## Context

Tock's Rust core must expose the same API to multiple platforms:

- **CLI:** Direct Rust binary (native, no FFI).
- **iOS/iPadOS/macOS:** SwiftUI apps requiring Swift bindings.
- **watchOS:** Minimal Swift API (subset of iOS API).
- **Web:** React/Next.js app requiring JavaScript bindings.

Hand-written FFI is error-prone and diverges across platforms. C-ABI FFI requires manual memory management and type bridging. We need automatic, type-safe bindings that feel idiomatic in each target language.

## Decision

**CLI:**
Native Rust binary (`tock-cli`) linking `tock-core` directly. Zero FFI overhead. Uses `clap` for argument parsing, `ratatui` for TUI, and `tokio` (single-threaded runtime) for async I/O (networking, filesystem watches).

**Apple platforms (iOS, iPadOS, macOS, watchOS):**
UniFFI 0.28+ generates Swift bindings from Rust:
- `tock-uniffi` crate exposes a high-level facade: `Workspace`, `TaskRepo`, `HabitRepo`, `TimeRepo`, `FocusController`, `SyncClient`.
- `.udl` files generated from `#[uniffi::export]` macros.
- Built as `staticlib` + `cdylib`; `xtask` script uses `lipo` to combine `arm64-ios`, `arm64-ios-sim`, `arm64-macos`, `x86_64-macos` into an `.xcframework`.
- UniFFI async support backed by a `tokio` current-thread runtime owned by the shim (core remains synchronous).
- watchOS gets a minimal subset (habit logging, timer start/stop) to fit memory and UX constraints.

**Web:**
- `tock-core` compiled to `wasm32-unknown-unknown` with `wasm-bindgen`.
- `tock-storage-web` provides IndexedDB storage (same trait as `rusqlite` storage).
- Sync uses `fetch` via `web-sys`.
- Distributed as an npm package.

**Build orchestration:**
`cargo xtask` scripts automate cross-compilation, lipo, xcframework packaging, WASM optimization, and npm publishing.

## Consequences

**Positive:**
- UniFFI eliminates manual FFI boilerplate (Swift bindings auto-generated from Rust types).
- WASM ensures the same core logic runs in browsers without reimplementation.
- Type safety enforced at compile time (Rust → Swift type errors caught by UniFFI codegen).
- Idiomatic APIs in each language (Swift feels like Swift, JS feels like JS).

**Negative:**
- UniFFI adds build complexity (udl generation, multi-arch linking).
- WASM binary size: ~2 MiB gzipped (acceptable for a productivity app; mitigated by lazy loading).
- UniFFI async support requires a hidden Tokio runtime in the shim (not in core, but present in the app).

**Neutral:**
- watchOS subset requires maintaining a smaller API surface (documented in `tock-uniffi/watchos.rs`).
- npm package versioning must stay synchronized with Rust crate versions (automated via CI).
