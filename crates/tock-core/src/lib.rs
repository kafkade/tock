//! # tock-core
//!
//! Pure-computation core for the tock productivity engine. This crate
//! has **zero I/O dependencies** (no filesystem, no networking, no async
//! runtime) so the same Rust code runs identically on native CLI, iOS
//! (via `UniFFI`), and the web (via WASM).
//!
//! See `docs/architecture.md` §4 and ADR-001 for the rationale and the
//! invariants that must be preserved.
//!
//! This is a foundation-phase placeholder: real domain types land in
//! later issues.

#![cfg_attr(not(feature = "std"), no_std)]

/// Version string of the tock-core library, matching the crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_matches_crate() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }
}
