# bindings/swift

Swift Package wrapping the tock Rust core via UniFFI. Provides:

- **TockFFI** — auto-generated Swift + C header from `tock-uniffi` (via
  `uniffi-bindgen`)
- **TockSwift** — idiomatic async/await wrapper layer for SwiftUI apps

## Prerequisites

- Rust toolchain with targets:
  `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `aarch64-apple-darwin`,
  `x86_64-apple-darwin`
- Xcode 15+ with Swift 5.9+
- `cargo` and the `uniffi-bindgen` tool (built from `tock-uniffi`)

## Building

### 1. Build the Rust library

```bash
# macOS (native, for development)
cargo build -p tock-uniffi

# iOS device
cargo build -p tock-uniffi --target aarch64-apple-ios

# iOS Simulator
cargo build -p tock-uniffi --target aarch64-apple-ios-sim
```

### 2. Generate Swift bindings

```bash
cargo run -p tock-uniffi --features cli --bin uniffi-bindgen -- \
    generate --library target/debug/libtock_uniffi.dylib \
    --language swift \
    --out-dir bindings/swift/Sources/TockFFI
```

This produces three files in `Sources/TockFFI/`:

- `tock_uniffi.swift` — Swift types and FFI function declarations
- `tock_uniffiFFI.h` — C header for the FFI functions
- `tock_uniffiFFI.modulemap` — Clang module map

### 3. Build the XCFramework (future)

An `xcframework` combining `arm64-ios`, `arm64-ios-sim`,
`arm64-macos`, and `x86_64-macos` slices will be automated via
`cargo xtask xcframework`. Until then, link the platform-specific
static library directly.

### 4. Use in your app

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
