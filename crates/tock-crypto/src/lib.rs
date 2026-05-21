//! # tock-crypto
//!
//! Cryptographic primitives for tock. Pure computation, no I/O, no
//! async, no `unsafe`. Built on audited [RustCrypto] and [dalek]
//! crates.
//!
//! ## Modules
//!
//! - [`aead`]    — AES-256-GCM authenticated encryption.
//! - [`kdf`]     — Argon2id password hashing and HKDF-SHA256 key derivation.
//! - [`keyexchange`] — X25519 Diffie-Hellman.
//! - [`signature`]   — Ed25519 signing and verification.
//! - [`secret`]  — `SecretBytes<N>` wrapper with zeroize-on-drop,
//!   constant-time equality, and redacted `Debug`.
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
pub mod error;
pub mod kdf;
pub mod keyexchange;
pub mod random;
pub mod secret;
pub mod signature;

pub use error::Error;
pub use secret::SecretBytes;
