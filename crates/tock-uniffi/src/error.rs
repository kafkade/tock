//! Error type exposed through the `UniFFI` boundary.
//!
//! Maps internal `tock-storage` and `tock-core` errors into a
//! Swift-friendly enum with safe, non-leaking messages.

/// Errors returned by the tock FFI layer.
///
/// Variants are chosen to give Swift callers enough granularity to
/// present useful UI without leaking cryptographic internals.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum TockError {
    /// The vault file was not found at the given path.
    #[error("vault not found at the specified path")]
    VaultNotFound,

    /// The password was wrong or the vault metadata was tampered with.
    #[error("invalid credentials or corrupted vault")]
    InvalidCredentials,

    /// The requested entity (task, project, habit, …) does not exist.
    #[error("entity not found")]
    NotFound,

    /// The workspace has been locked and cannot service requests.
    #[error("workspace is locked")]
    Locked,

    /// The operation is invalid for the entity's current state.
    #[error("invalid state: {message}")]
    InvalidState {
        /// Human-readable explanation.
        message: String,
    },

    /// A storage-layer failure (`SQLite`, I/O, migration).
    #[error("storage error: {message}")]
    StorageError {
        /// Human-readable explanation.
        message: String,
    },

    /// The caller supplied an invalid argument.
    #[error("invalid input: {message}")]
    InvalidInput {
        /// Human-readable explanation.
        message: String,
    },

    /// An unexpected internal error.
    #[error("internal error: {message}")]
    InternalError {
        /// Human-readable explanation.
        message: String,
    },
}

impl From<tock_storage::Error> for TockError {
    fn from(e: tock_storage::Error) -> Self {
        match e {
            tock_storage::Error::NotFound => Self::NotFound,
            tock_storage::Error::InvalidVaultOrCredentials => Self::InvalidCredentials,
            tock_storage::Error::UnsupportedVaultVersion => Self::StorageError {
                message: "vault requires a newer version of tock".into(),
            },
            tock_storage::Error::InvalidState(msg) => Self::InvalidState {
                message: msg.into(),
            },
            tock_storage::Error::MigrationChecksumMismatch { version } => Self::StorageError {
                message: format!("migration {version} checksum mismatch"),
            },
            other => Self::StorageError {
                message: other.to_string(),
            },
        }
    }
}

impl From<tock_sync::Error> for TockError {
    fn from(e: tock_sync::Error) -> Self {
        match e {
            tock_sync::Error::WireFormat(message) => Self::InvalidInput {
                message: message.into(),
            },
            other => Self::StorageError {
                message: other.to_string(),
            },
        }
    }
}
