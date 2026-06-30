//! Portable account credentials and a storage trait every client edge
//! implements (OS keyring on CLI, Keychain on Apple, `IndexedDB` on web).
//!
//! The password is intentionally absent — only the Secret Key, the SRP
//! session token, and its channel binding are retained, so a captured store
//! still cannot derive the URK without the user's password.

use serde::{Deserialize, Serialize};

/// Persisted account credentials. Sensitive fields (Secret Key, bearer token)
/// belong in a platform secret store; never serialise a password here.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountCredentials {
    /// Sign-in server URL.
    pub server_url: String,
    /// Account email / login.
    pub username: String,
    /// Server-assigned account id.
    pub account_id: String,
    /// Emergency-Kit Secret Key string (`A4-…`).
    pub secret_key: String,
    /// Hex bearer token for `Authorization: Bearer`.
    pub bearer_token: String,
    /// Hex channel-binding tag for `X-Tock-Channel-Binding`.
    pub channel_binding: String,
    /// Absolute session expiry (Unix seconds).
    pub expires_at: i64,
}

impl AccountCredentials {
    /// Whether the session is expired relative to `now` (Unix seconds).
    #[must_use]
    pub const fn is_expired(&self, now: i64) -> bool {
        self.expires_at <= now
    }
}

/// A platform credential store. Implementations perform the actual I/O.
pub trait CredentialStore {
    /// Concrete store error.
    type Error;

    /// Persist credentials, replacing any existing entry.
    ///
    /// # Errors
    /// Propagates the platform store error.
    fn save(&self, creds: &AccountCredentials) -> Result<(), Self::Error>;

    /// Load credentials, if present.
    ///
    /// # Errors
    /// Propagates the platform store error.
    fn load(&self) -> Result<Option<AccountCredentials>, Self::Error>;

    /// Remove stored credentials (logout).
    ///
    /// # Errors
    /// Propagates the platform store error.
    fn clear(&self) -> Result<(), Self::Error>;
}
