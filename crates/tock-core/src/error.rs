//! Top-level error type for `tock-core`.

use thiserror::Error as ThisError;

/// All errors produced by `tock-core`.
#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// Underlying cryptographic operation failed.
    #[cfg(feature = "vault")]
    #[error("crypto: {0}")]
    Crypto(#[from] tock_crypto::Error),

    /// Vault metadata is malformed, the wrap ciphertext didn't
    /// authenticate, or the supplied password is wrong.
    ///
    /// These three causes collapse into a single variant on purpose:
    /// distinguishing them would leak whether the metadata was tampered
    /// or the password was bad.
    #[error("invalid vault or credentials")]
    InvalidVaultOrCredentials,

    /// Vault format version is unsupported by this build.
    #[error(
        "unsupported vault format (file requires version {required}, this build supports {supported})"
    )]
    UnsupportedVaultVersion {
        /// Version the file requires.
        required: u16,
        /// Highest version this build understands.
        supported: u16,
    },

    /// A required vault header field is missing.
    #[error("missing vault header field: {0}")]
    MissingHeaderField(&'static str),

    /// The vault predates two-secret key derivation (format `v1`,
    /// password-only) and cannot be opened by this build. Pre-1.0 vaults
    /// have no automatic migration path and must be re-initialized.
    #[error(
        "vault uses the legacy password-only format (v{found}); re-initialize it (no automatic migration before 1.0)"
    )]
    VaultNeedsReinit {
        /// The legacy `format_version` found in the file.
        found: u16,
    },

    /// A canonical encoding could not be decoded.
    #[error("invalid encoding")]
    InvalidEncoding,

    /// Event signature did not verify, or the signer is not in the
    /// device registry.
    #[error("event signature invalid")]
    InvalidSignature,
}
