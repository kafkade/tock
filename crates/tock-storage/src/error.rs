//! Storage-layer errors.

use thiserror::Error as ThisError;

/// All errors produced by `tock-storage`.
#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// File system, `SQLite`, or other I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),

    /// `SQLite` returned an error.
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Vault file was not found at the given path.
    #[error("vault file not found")]
    NotFound,

    /// Requested operation is invalid for the current entity state.
    #[error("invalid state: {0}")]
    InvalidState(&'static str),

    /// Vault header was missing, malformed, didn't authenticate, or
    /// the wrong password was supplied. The three causes are
    /// intentionally indistinguishable to a caller.
    #[error("invalid vault or credentials")]
    InvalidVaultOrCredentials,

    /// Vault format is too new for this build.
    #[error("vault requires a newer version of tock")]
    UnsupportedVaultVersion,

    /// Vault uses the legacy password-only format (pre-2SKD) and must be
    /// re-initialized; there is no automatic migration before 1.0.
    #[error(
        "vault uses the legacy password-only format; re-initialize it (no automatic migration before 1.0)"
    )]
    VaultNeedsReinit,

    /// Migration framework refused to start: an already-applied
    /// migration's stored checksum no longer matches the embedded SQL.
    #[error("migration {version} checksum mismatch (developer/schema integrity check)")]
    MigrationChecksumMismatch {
        /// Migration version that failed the consistency check.
        version: u32,
    },

    /// Generic propagation from `tock-core`.
    #[error("core: {0}")]
    Core(#[from] tock_core::Error),

    /// Underlying cryptographic failure.
    #[error("crypto: {0}")]
    Crypto(#[from] tock_crypto::Error),

    /// Event log read encountered an event whose payload AEAD or
    /// signature did not verify.
    #[error("event log integrity check failed")]
    EventLogIntegrity,
}
