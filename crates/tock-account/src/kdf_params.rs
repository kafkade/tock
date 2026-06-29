//! Serializable KDF parameters and Unlock Root Key derivation.
//!
//! The server stores [`KdfParams`] verbatim (as opaque JSON) at registration
//! and echoes them back in the unauthenticated `srp/start` response so a fresh
//! device can re-derive its Unlock Root Key (URK) — and thus the SRP private
//! input — *before* it has any session to download the full vault header.

use serde::{Deserialize, Serialize};
use tock_core::vault::VaultHeader;
use tock_crypto::SecretBytes;
use tock_crypto::SecretKey;
use tock_crypto::kdf::{Argon2Params, derive_unlock_root_key};

use crate::error::AccountError;

/// Non-secret 2SKD parameters needed to derive the URK from a password and
/// the account Secret Key.
///
/// None of these are confidential — the URK is only recoverable to someone
/// holding *both* the password and the Secret Key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Account id (UUID bytes) bound into the Secret-Key HKDF step.
    pub account_id: [u8; 16],
    /// Argon2id salt for the `password → MK` step.
    pub kdf_salt: [u8; 16],
    /// Two-secret KDF version (selects Argon2 params + 2SKD labels).
    pub kdf_version: u16,
    /// Argon2 iterations.
    pub argon2_t: u32,
    /// Argon2 memory cost (KiB).
    pub argon2_m_kib: u32,
    /// Argon2 parallelism.
    pub argon2_p: u32,
}

impl KdfParams {
    /// Extract the parameters from a freshly initialised vault header.
    #[must_use]
    pub const fn from_header(header: &VaultHeader) -> Self {
        Self {
            account_id: *header.account_id.as_bytes(),
            kdf_salt: header.kdf_salt,
            kdf_version: header.kdf_version,
            argon2_t: header.argon2.t,
            argon2_m_kib: header.argon2.m_kib,
            argon2_p: header.argon2.p,
        }
    }

    /// Parse from the `serde_json::Value` carried in the `srp/start` reply.
    ///
    /// # Errors
    /// Returns [`AccountError::KdfParams`] when the value is null or its
    /// shape does not match.
    pub fn from_json(value: &serde_json::Value) -> Result<Self, AccountError> {
        if value.is_null() {
            return Err(AccountError::KdfParams);
        }
        serde_json::from_value(value.clone()).map_err(|_| AccountError::KdfParams)
    }

    /// Serialise to a `serde_json::Value` for the register request.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    /// Derive the Unlock Root Key from the password and Secret Key.
    ///
    /// # Errors
    /// Returns [`AccountError::Crypto`] if Argon2/HKDF fails or the stored
    /// Argon2 parameters are out of bounds.
    pub fn derive_urk(
        &self,
        password: &[u8],
        secret_key: &SecretKey,
    ) -> Result<SecretBytes<32>, AccountError> {
        let params = Argon2Params::new(self.argon2_t, self.argon2_m_kib, self.argon2_p)?;
        let urk = derive_unlock_root_key(
            password,
            secret_key.expose_secret(),
            &self.kdf_salt,
            &self.account_id,
            self.kdf_version,
            params,
        )?;
        Ok(urk)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::KdfParams;

    fn sample() -> KdfParams {
        KdfParams {
            account_id: [0x11; 16],
            kdf_salt: [0x22; 16],
            kdf_version: 1,
            argon2_t: 1,
            argon2_m_kib: 8,
            argon2_p: 1,
        }
    }

    #[test]
    fn json_round_trips() {
        let p = sample();
        assert_eq!(KdfParams::from_json(&p.to_json()).expect("parse"), p);
    }

    #[test]
    fn null_json_rejected() {
        assert!(KdfParams::from_json(&serde_json::Value::Null).is_err());
    }
}
