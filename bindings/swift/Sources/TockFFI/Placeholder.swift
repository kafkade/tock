// This directory holds UniFFI-generated files.
//
// After building the Rust library, regenerate with:
//
//     cargo run -p tock-uniffi --features cli --bin uniffi-bindgen -- \
//         generate --library target/debug/libtock_uniffi.dylib \
//         --language swift \
//         --out-dir bindings/swift/Sources/TockFFI
//
// Generated files (git-ignored via /bindings/swift/generated/ rule):
//   - tock_uniffi.swift
//   - tock_uniffiFFI.h
//   - tock_uniffiFFI.modulemap
//
// This placeholder ensures the Swift package can reference the target
// directory even before generation has run.

// Intentionally empty — generated code goes here.
