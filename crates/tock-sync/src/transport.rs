//! Sync transport types and trait.
//!
//! Defines the [`Transport`] trait (architecture §6.6) and the
//! associated data types: [`SyncCursor`], [`PushAck`], [`PullBatch`],
//! [`SnapshotId`], [`EncryptedSnapshot`], and [`OnboardingBlob`].
//!
//! The trait is currently **synchronous** because the workspace has no
//! async runtime. When transport implementations land (HTTP, WebSocket,
//! LAN, file sync), this will evolve to `async_trait` — consumers
//! should design for that.
//!
//! Implementations live outside `tock-sync` (in platform-specific
//! crates) and are injected via the trait.

use time::OffsetDateTime;
use tock_core::event::{DeviceId, SignedEvent, VectorClock};
use tock_crypto::keyexchange::PublicKey;
use uuid::Uuid;

use crate::Error;

/// Opaque cursor for delta-sync: tracks the last-seen vector clock so
/// the server can return only events after that point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncCursor {
    /// Vector clock snapshot at the time of the last successful pull.
    pub vector_clock: VectorClock,
}

impl SyncCursor {
    /// Construct from a vector clock.
    #[must_use]
    pub const fn new(vector_clock: VectorClock) -> Self {
        Self { vector_clock }
    }

    /// Empty cursor (fresh start).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            vector_clock: VectorClock::new(),
        }
    }
}

/// Server acknowledgement after a push.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushAck {
    /// Number of events the server accepted (new).
    pub accepted: usize,
    /// Number of events the server already had (deduplicated).
    pub duplicates: usize,
    /// Server's highest Lamport timestamp after processing.
    pub server_lamport: u64,
}

/// A batch of events returned by a pull.
#[derive(Clone, Debug)]
pub struct PullBatch {
    /// Events in this batch, in causal order.
    pub events: Vec<SignedEvent>,
    /// Cursor for the next pull (start from here).
    pub next_cursor: SyncCursor,
    /// Whether there are more events to pull after this batch.
    pub more: bool,
}

/// Snapshot identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SnapshotId(pub Uuid);

/// An encrypted snapshot blob for cold onboarding.
#[derive(Clone, Debug)]
pub struct EncryptedSnapshot {
    /// Snapshot identifier.
    pub id: SnapshotId,
    /// Highest event id included in this snapshot.
    pub upto_event_id: Uuid,
    /// AEAD-encrypted snapshot body.
    pub blob_ct: Vec<u8>,
    /// AEAD nonce.
    pub blob_nonce: [u8; 12],
    /// When this snapshot was created.
    pub created_at: OffsetDateTime,
}

/// Encrypted vault-key blob sent from an existing device to a new one
/// during the device pairing flow (architecture §6.5).
#[derive(Clone, Debug)]
pub struct OnboardingBlob {
    /// Target device that should receive this blob.
    pub target_device: DeviceId,
    /// AEAD-encrypted vault key (ciphertext + tag).
    pub encrypted_vk: Vec<u8>,
    /// AEAD nonce used for the wrap.
    pub nonce: [u8; 12],
    /// Ephemeral X25519 public key of the inviter, so the accepter
    /// can compute the shared secret.
    pub ephemeral_pubkey: PublicKey,
    /// When this blob was created (for expiry checking).
    pub created_at: OffsetDateTime,
}

/// Sync transport abstraction.
///
/// Implementations handle the actual network/file I/O: HTTP + REST,
/// WebSocket, LAN (mDNS), or file-based (Syncthing / iCloud Drive).
///
/// Currently synchronous; will evolve to `async_trait` when the first
/// transport implementation lands.
pub trait Transport: Send + Sync {
    /// Push a batch of signed events to the server.
    ///
    /// # Errors
    /// Implementation-specific I/O or protocol errors.
    fn push(&self, events: &[SignedEvent]) -> Result<PushAck, Error>;

    /// Pull events after `cursor`, returning at most `limit` events.
    ///
    /// # Errors
    /// Implementation-specific I/O or protocol errors.
    fn pull(&self, cursor: &SyncCursor, limit: usize) -> Result<PullBatch, Error>;

    /// Fetch a snapshot by id for cold onboarding.
    ///
    /// # Errors
    /// Implementation-specific I/O or protocol errors.
    fn fetch_snapshot(&self, id: SnapshotId) -> Result<EncryptedSnapshot, Error>;

    /// Store an onboarding blob for a target device.
    ///
    /// # Errors
    /// Implementation-specific I/O or protocol errors.
    fn put_onboarding_blob(
        &self,
        target_device: DeviceId,
        blob: OnboardingBlob,
    ) -> Result<(), Error>;

    /// Retrieve a pending onboarding blob for this device.
    ///
    /// Returns `None` if no blob is waiting.
    ///
    /// # Errors
    /// Implementation-specific I/O or protocol errors.
    fn get_onboarding_blob(&self, device: DeviceId) -> Result<Option<OnboardingBlob>, Error>;
}
