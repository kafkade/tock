// This directory holds the UniFFI-generated Swift bindings.
//
// `Sources/TockFFI/tock_uniffi.swift` is emitted by `cargo xtask xcframework`
// (gitignored). That same command also compiles the Rust static libraries
// for the Apple targets and packages them as `TockFFI.xcframework`.
//
// Regenerate both with:
//
//     cargo xtask xcframework
//
// This `Placeholder.swift` keeps the SwiftPM `TockFFI` target's source
// directory tracked before generation has run; it is intentionally empty.
