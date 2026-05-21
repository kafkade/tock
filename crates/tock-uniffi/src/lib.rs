//! # tock-uniffi
//!
//! `UniFFI` scaffolding crate exposing the tock core API to Swift for
//! iOS, iPadOS, macOS, and watchOS apps.
//!
//! Per ADR-005, this crate owns the `unsafe` boundary for the workspace:
//! `UniFFI`-generated code uses `#[no_mangle] pub extern "C"` and
//! `unsafe` blocks. All other crates `forbid` `unsafe_code`.
//!
//! Foundation-phase placeholder.

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(tock_core::VERSION, env!("CARGO_PKG_VERSION"));
    }
}
