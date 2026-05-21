//! # tock-crypto
//!
//! Cryptographic primitives for tock: envelope encryption, key hierarchy,
//! Argon2id KDF, SRP-6a verifier seeding, and recovery codes.
//!
//! Pure computation only — no I/O. All key types implement [`Zeroize`]
//! (added in a later phase) and `Debug` impls redact secret values.
//!
//! See `docs/architecture.md` §5 and ADR-002 for the cryptographic design.
//!
//! Foundation-phase placeholder.
//!
//! [`Zeroize`]: https://docs.rs/zeroize

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(2 + 2, 4);
    }
}
