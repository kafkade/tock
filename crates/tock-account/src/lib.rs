//! Zero-I/O account signup/login orchestration shared by every tock client.
//!
//! This crate wires together the existing `tock-crypto` two-secret KDF and
//! SRP-6a primitives and the `tock-core` vault header into the account flow
//! the `tock-server` already serves. It performs **no I/O** (per ADR-001):
//! every client edge (CLI/reqwest, Apple/URLSession, web/fetch) supplies the
//! HTTP and credential storage; this layer just produces and consumes the
//! request/response structs so all three speak the identical wire protocol.
//!
//! - [`signup`] — registration material, Emergency Kit, Setup Code.
//! - [`login`] — SRP client state machine + DTOs + session material.
//! - [`kdf_params`] — re-derivable KDF parameters → Unlock Root Key.
//! - [`credentials`] — portable credential struct + storage trait.

pub mod codec;
pub mod credentials;
pub mod error;
pub mod kdf_params;
pub mod login;
pub mod rotate;
pub mod signup;

pub use credentials::{AccountCredentials, CredentialStore};
pub use error::AccountError;
pub use kdf_params::KdfParams;
pub use login::{
    FinishRequest, FinishResponse, LoginPending, LoginStart, SessionMaterial, StartRequest,
    StartResponse,
};
pub use rotate::{RotatePasswordMaterial, SrpVerifierUpdate};
pub use signup::{
    EmergencyKit, RegisterRequest, RegisterResponse, SRP_GROUP, SetupCode, SignupMaterial,
};

/// Re-export the account Secret Key type so client edges parse `A4-…` strings
/// without depending on `tock-crypto` directly.
pub use tock_crypto::SecretKey;
