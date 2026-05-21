# tock-uniffi

UniFFI scaffolding crate exposing tock to Swift (iOS, iPadOS, macOS,
watchOS).

Licensed under [Apache-2.0](../../LICENSE-APACHE). See ADR-005.

This crate isolates all FFI `unsafe` code — every other crate in the
workspace forbids `unsafe_code`.
