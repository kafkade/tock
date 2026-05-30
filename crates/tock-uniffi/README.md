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

```bash
# Build the bindgen CLI
cargo build -p tock-uniffi --features cli --bin uniffi-bindgen

# Generate Swift + header + modulemap
cargo run -p tock-uniffi --features cli --bin uniffi-bindgen -- \
    generate --library target/debug/libtock_uniffi.dylib \
    --language swift \
    --out-dir bindings/swift/Sources/TockFFI
```

See [`bindings/swift/README.md`](../../bindings/swift/README.md) for the
full Swift package build instructions.
