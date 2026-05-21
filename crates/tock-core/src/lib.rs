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
//! ## Feature flags
//!
//! - `std` (default): enables standard-library features. The crate
//!   currently always requires `std`; the flag exists for forward
//!   compatibility.
//! - `core`: pure-data surface (no crypto, no vault types). Used by
//!   the WASM CI smoke build to keep the bundle small.
//! - `vault` (default): vault header, key hierarchy, event types, and
//!   the [`event`] / [`vault`] modules. Depends on `tock-crypto`.

/// Version string of the tock-core library, matching the crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "vault")]
pub mod domain;
pub mod error;

#[cfg(feature = "vault")]
pub mod event;

#[cfg(feature = "vault")]
pub mod vault;

pub use error::Error;

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_matches_crate() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }
}
