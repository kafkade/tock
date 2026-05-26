//! Sync-layer error types.

use thiserror::Error as ThisError;

/// All errors produced by `tock-sync`.
#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// Wire format data is malformed or truncated.
    #[error("wire format error: {0}")]
    WireFormat(&'static str),

    /// A conflict requires user review before it can be resolved.
    /// Contains the entity id and the two conflicting event ids.
    #[error("conflict on entity {entity_id} requires review")]
    ConflictRequiresReview {
        /// Entity that has conflicting events.
        entity_id: uuid::Uuid,
        /// The local head event id.
        local_event_id: uuid::Uuid,
        /// The incoming remote event id.
        remote_event_id: uuid::Uuid,
    },

    /// The pairing invitation has expired (5-minute window).
    #[error("pairing invitation expired")]
    PairingExpired,

    /// The out-of-band fingerprint does not match the peer's public key.
    #[error("pairing fingerprint mismatch")]
    PairingFingerprintMismatch,

    /// The pairing secret has already been consumed.
    #[error("pairing secret already consumed")]
    PairingSecretConsumed,

    /// Underlying cryptographic operation failed.
    #[error("crypto: {0}")]
    Crypto(#[from] tock_crypto::Error),

    /// Error from `tock-core`.
    #[error("core: {0}")]
    Core(#[from] tock_core::Error),
}
