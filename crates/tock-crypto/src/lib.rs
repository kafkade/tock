//! # tock-crypto
//!
//! Cryptographic primitives for tock. Pure computation, no I/O, no
//! async, no `unsafe`. Built on audited [RustCrypto] and [dalek]
//! crates.
//!
//! ## Modules
//!
//! - [`aead`]    — AES-256-GCM authenticated encryption.
//! - [`base32`]  — Crockford Base32 for human-transcribable secrets.
//! - [`kdf`]     — Argon2id password hashing, HKDF-SHA256 key derivation,
//!   and two-secret key derivation (2SKD).
//! - [`keyexchange`] — X25519 Diffie-Hellman.
//! - [`signature`]   — Ed25519 signing and verification.
//! - [`srp`]     — SRP-6a (RFC 5054) zero-knowledge auth over the
//!   two-secret (password + Secret Key) input.
//! - [`secret`]  — `SecretBytes<N>` wrapper with zeroize-on-drop,
//!   constant-time equality, and redacted `Debug`.
//! - [`secret_key`] — account `SecretKey` (the "something you have"
//!   factor) and its Emergency-Kit encoding.
//! - [`random`]  — fallible OS RNG helper.
//! - [`error`]   — crate-wide [`Error`] enum.
//!
//! All public functions that touch the OS RNG are fallible
//! (`try_random` / `try_generate`) because the workspace forbids
//! panicking code.
//!
//! See [ADR-002] and architecture §5 for the cryptographic design that
//! drives the API shapes here.
//!
//! [RustCrypto]: https://github.com/RustCrypto
//! [dalek]: https://github.com/dalek-cryptography
//! [ADR-002]: https://github.com/kafkade/tock/blob/main/docs/adr/ADR-002-end-to-end-encryption.md

#![forbid(unsafe_code)]

pub mod aead;
pub mod base32;
pub mod error;
pub mod kdf;
pub mod keyexchange;
pub mod random;
pub mod secret;
pub mod secret_key;
pub mod signature;
pub mod srp;

pub use error::Error;
pub use secret::SecretBytes;
pub use secret_key::SecretKey;
