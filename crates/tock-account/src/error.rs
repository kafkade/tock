//! Error type for the zero-I/O account orchestration layer.

/// Errors produced while orchestrating account signup/login.
///
/// Variants never carry secret material; failed credential checks collapse
/// to a single opaque [`AccountError::Auth`] so callers cannot distinguish
/// a wrong password from a wrong Secret Key.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AccountError {
    /// A wrapped cryptographic primitive failed (Argon2/HKDF/SRP/RNG).
    #[error("crypto error")]
    Crypto,
    /// Authentication failed (bad credentials or server proof mismatch).
    #[error("authentication failed")]
    Auth,
    /// A wire value could not be decoded (bad base64, length, or shape).
    #[error("invalid encoding: {0}")]
    Encoding(&'static str),
    /// The Setup Code could not be parsed.
    #[error("invalid setup code")]
    SetupCode,
    /// KDF parameters returned by the server were missing or malformed.
    #[error("missing or malformed kdf params")]
    KdfParams,
}

impl From<tock_crypto::Error> for AccountError {
    fn from(_: tock_crypto::Error) -> Self {
        Self::Crypto
    }
}

impl From<tock_core::Error> for AccountError {
    fn from(_: tock_core::Error) -> Self {
        Self::Crypto
    }
}
