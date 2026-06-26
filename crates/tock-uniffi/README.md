# tock-uniffi

UniFFI scaffolding crate exposing tock to Swift (iOS, iPadOS, macOS,
watchOS).

Licensed under [Apache-2.0](../../LICENSE-APACHE). See ADR-005.

This crate isolates all FFI `unsafe` code — every other crate in the
workspace forbids `unsafe_code`.

## API

The entry point is `Workspace`, an opaque UniFFI object wrapping an
unlocked vault. All domain operations are methods on `Workspace`:

- **Tasks**: add, get, list, modify, complete, cancel, delete
- **Projects**: add, get, list
- **Areas**: add, list
- **Tags**: list
- **Time blocks**: start, stop, current, resume, list
- **Focus sessions**: start, status, complete cycle, skip break, pause,
  resume, abort, finish
- **Habits**: add, get, list, log entry, archive

Free functions `init_workspace` and `open_workspace` create/open vaults.

## Building

```bash
cargo build -p tock-uniffi
cargo test -p tock-uniffi
```

## Generating Swift bindings

The Swift bindings and the Apple `TockFFI.xcframework` are produced by the
`xtask` orchestrator, which builds the FFI crate for every Apple target,
runs `uniffi-bindgen`, and assembles the framework:

```bash
cargo xtask xcframework
```

To run `uniffi-bindgen` directly against the host library (e.g. for
debugging the generated output):

```bash
# Build the host library + bindgen CLI
cargo build -p tock-uniffi --features cli

# Generate Swift + header + modulemap
cargo run -p tock-uniffi --features cli --bin uniffi-bindgen -- \
    generate --library target/debug/libtock_uniffi.dylib \
    --language swift \
    --out-dir <out-dir>
```

See [`bindings/swift/README.md`](../../bindings/swift/README.md) and
[`xtask/README.md`](../../xtask/README.md) for the full Swift package build
instructions.
