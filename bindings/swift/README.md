# bindings/swift

Swift Package wrapping the tock Rust core via UniFFI. Provides:

- **TockFFI** — auto-generated Swift + C header from `tock-uniffi` (via
  `uniffi-bindgen`)
- **TockSwift** — idiomatic async/await wrapper layer for SwiftUI apps

## Prerequisites

- Rust toolchain (the workspace pin in `rust-toolchain.toml`)
- Xcode 15+ with Swift 5.9+ (`swift`, `xcodebuild`, `lipo` on `PATH`)
- The Apple Rust targets — `cargo xtask xcframework` installs these for you
  via `rustup target add`:
  `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `aarch64-apple-darwin`,
  `x86_64-apple-darwin`

## Building

### One command: generate bindings + XCFramework

```bash
cargo xtask xcframework
```

This single task (see `xtask/src/main.rs`):

1. Installs the four Apple Rust targets (idempotent).
2. Builds `tock-uniffi` for each target with the `apple-ffi` profile.
3. Runs `uniffi-bindgen` and writes the Swift bindings to
   `Sources/TockFFI/tock_uniffi.swift`.
4. `lipo`s the two macOS slices and runs `xcodebuild -create-xcframework`
   to produce `TockFFI.xcframework`.

Both `tock_uniffi.swift` and `TockFFI.xcframework/` are **gitignored** — they
are build outputs. Run the task once before `swift build`/`swift test`.

> **Why a dedicated `apple-ffi` Cargo profile?** The workspace `release`
> profile sets `strip = "symbols"` and `lto = "thin"`, both of which drop the
> `#[no_mangle]` UniFFI scaffolding from the static library and break linkage.
> `apple-ffi` inherits `release` but disables stripping and LTO.

### Run the tests

```bash
cd bindings/swift
swift test
```

The tests in `Tests/TockSwiftTests` open an encrypted vault and round-trip
tasks, projects, areas, tags, time blocks, focus sessions, and habits through
the real Rust core (macOS / arm64 slice).

### Use in your app

```swift
// Package.swift dependency
.package(path: "../bindings/swift")

// Import in your Swift code
import TockSwift
```

## Architecture

See [`docs/adr/ADR-005-platform-bindings.md`](../../docs/adr/ADR-005-platform-bindings.md)
and [`docs/architecture.md`](../../docs/architecture.md) §4.3, §8.1.

### API Surface

The UniFFI facade exposes a `Workspace` object with methods for:

| Domain          | Operations                                                |
|-----------------|-----------------------------------------------------------|
| **Tasks**       | add, get, list, modify, complete, cancel, delete          |
| **Projects**    | add, get, list                                            |
| **Areas**       | add, list                                                 |
| **Tags**        | list                                                      |
| **Time blocks** | start, stop, current, resume, list                        |
| **Focus**       | start, status, complete cycle, skip break, pause, resume, |
|                 | abort, finish                                             |
| **Habits**      | add, get, list, log entry, archive                        |

### Type Mapping

| Rust type         | UniFFI / Swift type       |
|-------------------|---------------------------|
| `Uuid`            | `String` (hyphenated)     |
| `OffsetDateTime`  | `String` (RFC 3339)       |
| `BTreeMap<…>`     | `String` (JSON)           |
| Domain enums      | Swift enums               |
| Domain structs    | Swift structs (records)   |
| `OpenVault`       | Opaque `Workspace` object |

### Async (future)

The current FFI API is synchronous. The `TockSwift` wrapper dispatches
calls onto a background `DispatchQueue`. A future version will use
UniFFI's native async support with a Tokio runtime in the Rust shim
(per ADR-005 §4.3.1).

## License

[Apache-2.0](../../LICENSE-APACHE)
