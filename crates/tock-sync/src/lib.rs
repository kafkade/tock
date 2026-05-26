//! # tock-sync
//!
//! Event-sourced synchronization for tock: wire format, conflict
//! resolution with user review, transport trait, device pairing, and
//! a stateless sync engine.
//!
//! All code in this crate is **pure computation** — no I/O, no async
//! runtime. Transport implementations live in platform-specific crates
//! (`tock-cli` uses `reqwest`, web uses `fetch`, etc.) and are injected
//! via the [`transport::Transport`] trait.
//!
//! See `docs/architecture.md` §6 and ADR-003 for the event-sourced
//! sync design.
//!
//! ## Modules
//!
//! - [`wire`]      — Binary wire format for [`SignedEvent`] batches +
//!   encrypted batch envelope.
//! - [`transport`] — Sync types ([`SyncCursor`], [`PushAck`],
//!   [`PullBatch`], etc.) and the [`Transport`] trait.
//! - [`conflict`]  — Conflict detection (vector clocks) and resolution
//!   (field-level merge, LWW, configurable policies).
//! - [`pairing`]   — Device pairing via X25519 key exchange with
//!   fingerprint verification and onboarding blob.
//! - [`engine`]    — Stateless sync engine that processes incoming
//!   events against local heads.
//!
//! [`SignedEvent`]: tock_core::event::SignedEvent
//! [`SyncCursor`]: transport::SyncCursor
//! [`PushAck`]: transport::PushAck
//! [`PullBatch`]: transport::PullBatch
//! [`Transport`]: transport::Transport

pub mod conflict;
pub mod engine;
pub mod error;
pub mod pairing;
pub mod recovery;
pub mod revocation;
pub mod transport;
pub mod wire;

pub use error::Error;

// Re-export key types for convenience.
pub use conflict::{ConflictEntry, ConflictPolicy, ConflictResolution, EventRelation};
pub use engine::{IngestAction, IngestResult};
pub use pairing::{PairingInvite, PairingSecret};
pub use recovery::{RecoveryKey, RotationPlan};
pub use revocation::{DeviceRegistryEntry, DeviceStatus, RevocationRecord};
pub use transport::{
    EncryptedSnapshot, OnboardingBlob, PullBatch, PushAck, SnapshotId, SyncCursor, Transport,
};
