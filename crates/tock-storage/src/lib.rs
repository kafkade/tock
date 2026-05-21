//! # tock-storage
//!
//! `SQLite` storage adapter for tock. Implements the storage traits
//! defined in [`tock_core`] using `rusqlite` (with `SQLCipher` app-layer
//! encryption in later phases).
//!
//! See `docs/architecture.md` §4 and ADR-004 for the storage design.
//!
//! Foundation-phase placeholder.

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(tock_core::VERSION, env!("CARGO_PKG_VERSION"));
    }
}
