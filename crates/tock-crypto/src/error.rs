//! Crate-wide error type.
//!
//! Error variants are intentionally coarse: distinguishing *why* an
//! AEAD authentication tag failed (truncation vs. flipped bit vs. wrong
//! key) gives an attacker a side-channel. Every failure path that
//! could otherwise leak partial information returns the same
//! [`Error::Aead`] variant.
//!
//! No variant carries secret material; all `Debug` / `Display`
//! output is safe to log.

use thiserror::Error as ThisError;

/// All errors that can be produced by `tock-crypto`.
#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// Authenticated decryption failed. May indicate a wrong key,
    /// wrong nonce, wrong AAD, truncated ciphertext, or a bit flip.
    /// Intentionally not differentiated.
    #[error("aead authentication failed")]
    Aead,

    /// AEAD encryption failed (e.g. plaintext larger than the maximum
    /// message size for the underlying cipher).
    #[error("aead encryption failed")]
    AeadEncrypt,

    /// Operating system RNG could not produce randomness.
    #[error("operating system rng failure")]
    Rng,

    /// Argon2id parameters were rejected (out of bounds, invalid
    /// combination, or KDF output buffer too small / too large).
    #[error("invalid argon2 parameters")]
    InvalidArgon2Params,

    /// Argon2id hashing failed.
    #[error("argon2 hash failure")]
    Argon2,

    /// HKDF expansion failed (output buffer too long for the chosen hash).
    #[error("hkdf expansion failure")]
    Hkdf,

    /// A byte slice could not be interpreted as the expected
    /// fixed-size cryptographic value (e.g. invalid Ed25519 public
    /// point, wrong-length input).
    #[error("invalid encoding of cryptographic value")]
    InvalidEncoding,

    /// Diffie-Hellman produced the all-zero shared secret, indicating
    /// a low-order or otherwise contributory peer public key.
    #[error("diffie-hellman produced contributory (all-zero) shared secret")]
    ContributorySharedSecret,

    /// Ed25519 signature verification failed.
    #[error("signature verification failed")]
    SignatureVerification,

    /// An account Secret Key string was malformed: wrong format/version
    /// tag, wrong length, or a failed transcription checksum. Carries no
    /// secret material.
    #[error("invalid account secret key")]
    InvalidSecretKey,
}
