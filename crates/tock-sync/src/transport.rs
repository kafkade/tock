//! Sync transport types and trait.
//!
//! Defines the async [`Transport`] trait (architecture §6.6) and the
//! associated data types: [`SyncCursor`], [`PushAck`], [`PullBatch`],
//! [`SnapshotId`], [`EncryptedSnapshot`], and [`OnboardingBlob`].
//!
//! The trait is `async` (via [`async_trait`]). `tock-sync` itself adds
//! **no async runtime** — `async-trait` only desugars `async fn` in
//! traits and is WASM-safe. Concrete transports (HTTP, WebSocket, LAN,
//! file sync) live outside `tock-sync` in platform crates and bring
//! their own runtime; per ADR-001 the CLI's HTTP transport lives in
//! `tock-cli`, the only Rust crate allowed to do network I/O.

use async_trait::async_trait;
use time::OffsetDateTime;
use tock_core::event::{DeviceId, SignedEvent};
use tock_crypto::keyexchange::PublicKey;
use uuid::Uuid;

use crate::Error;

/// Opaque, monotonic server cursor for delta-sync.
///
/// The server assigns each stored event a monotonically increasing
/// position (insertion order). A pull returns every event with a
/// position greater than the cursor, which guarantees completeness even
/// when devices push out of Lamport order (e.g. an offline device whose
/// per-device Lamport lags behind the rest of the vault).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SyncCursor {
    /// Highest server position consumed so far.
    pub position: u64,
}

impl SyncCursor {
    /// Cursor for a fresh start (pull everything).
    #[must_use]
    pub const fn start() -> Self {
        Self { position: 0 }
    }

    /// Cursor at a specific server position.
    #[must_use]
    pub const fn at(position: u64) -> Self {
        Self { position }
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
    /// Events in this batch, in server-assigned order.
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

impl OnboardingBlob {
    /// Serialize to a self-describing byte frame for transport.
    ///
    /// Layout: `target_device[16] | nonce[12] | ephemeral_pubkey[32] |
    /// created_at_unix[i64 BE, 8] | vk_len[u32 BE, 4] | encrypted_vk`.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + 12 + 32 + 8 + 4 + self.encrypted_vk.len());
        out.extend_from_slice(self.target_device.as_bytes());
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(self.ephemeral_pubkey.as_bytes());
        out.extend_from_slice(&self.created_at.unix_timestamp().to_be_bytes());
        let len = u32::try_from(self.encrypted_vk.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&self.encrypted_vk);
        out
    }

    /// Parse a frame produced by [`OnboardingBlob::encode`].
    ///
    /// # Errors
    /// [`Error::WireFormat`] if the frame is truncated or malformed.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        const HEADER: usize = 16 + 12 + 32 + 8 + 4;
        if bytes.len() < HEADER {
            return Err(Error::WireFormat("onboarding blob too short"));
        }
        let mut target = [0_u8; 16];
        target.copy_from_slice(&bytes[0..16]);
        let mut nonce = [0_u8; 12];
        nonce.copy_from_slice(&bytes[16..28]);
        let mut pk = [0_u8; 32];
        pk.copy_from_slice(&bytes[28..60]);
        let mut ts = [0_u8; 8];
        ts.copy_from_slice(&bytes[60..68]);
        let mut len_b = [0_u8; 4];
        len_b.copy_from_slice(&bytes[68..72]);
        let vk_len = u32::from_be_bytes(len_b) as usize;
        if bytes.len() - HEADER != vk_len {
            return Err(Error::WireFormat("onboarding blob length mismatch"));
        }
        let encrypted_vk = bytes[HEADER..].to_vec();
        let created_at = OffsetDateTime::from_unix_timestamp(i64::from_be_bytes(ts))
            .map_err(|_| Error::WireFormat("onboarding blob bad timestamp"))?;
        Ok(Self {
            target_device: DeviceId::from_bytes(target),
            encrypted_vk,
            nonce,
            ephemeral_pubkey: PublicKey::from_bytes(pk),
            created_at,
        })
    }
}

/// Sync transport abstraction.
///
/// Implementations handle the actual network/file I/O: HTTP + REST,
/// WebSocket, LAN (mDNS), or file-based (Syncthing / iCloud Drive).
#[async_trait]
pub trait Transport: Send + Sync {
    /// Register this device's verifying key with the server so peers
    /// can later look it up (idempotent).
    ///
    /// # Errors
    /// Implementation-specific I/O or protocol errors.
    async fn register_device(
        &self,
        device_id: DeviceId,
        verifying_key: &[u8; 32],
        label: Option<&str>,
    ) -> Result<(), Error>;

    /// Push a batch of signed events to the server.
    ///
    /// # Errors
    /// Implementation-specific I/O or protocol errors.
    async fn push(&self, events: &[SignedEvent]) -> Result<PushAck, Error>;

    /// Pull events after `cursor`, returning at most `limit` events.
    ///
    /// # Errors
    /// Implementation-specific I/O or protocol errors.
    async fn pull(&self, cursor: SyncCursor, limit: usize) -> Result<PullBatch, Error>;

    /// Store an onboarding blob for a target device.
    ///
    /// # Errors
    /// Implementation-specific I/O or protocol errors.
    async fn put_onboarding_blob(
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
    async fn get_onboarding_blob(&self, device: DeviceId) -> Result<Option<OnboardingBlob>, Error>;
}
