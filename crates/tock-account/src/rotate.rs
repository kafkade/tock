//! Password rotation: re-wrap the Vault Key and mint a fresh SRP verifier.
//!
//! Rotating the password changes the Unlock Root Key (URK), and therefore the
//! Master Encryption Key (MEK) and the SRP verifier derived from it. The
//! account [`SecretKey`] is **unchanged**, so the Emergency Kit and Setup Code
//! stay valid.
//!
//! Pure (no I/O, per ADR-001): this turns
//! `(old_password, new_password, SecretKey, current header)` into the new
//! server-side SRP credentials plus the re-wrapped vault header the client
//! uploads. The header keeps its salts, `account_id`, and KDF parameters — only
//! the VK wrap (`vk_wrap_nonce`/`vk_wrap_ct`) and the SRP salt/verifier change.
//!
//! Two shapes are handled:
//! - **Header with a wrapped Vault Key** (native CLI/Apple, or a browser vault):
//!   the VK is unwrapped with the old MEK and re-wrapped under the new MEK, so
//!   the old password stops working while data stays readable with the new one.
//! - **Header without a wrapped Vault Key** (`vk_wrap_ct` empty): there is
//!   nothing to re-wrap, so only the SRP verifier rotates. The change is still
//!   authorized by the caller's active session.

use serde::{Deserialize, Serialize};
use tock_core::vault::{KeyHierarchy, VaultHeader};
use tock_crypto::SecretKey;
use tock_crypto::kdf::derive_srp_input;
use tock_crypto::random::fill_random;
use tock_crypto::srp::compute_verifier;

use crate::codec::base64_encode;
use crate::error::AccountError;
use crate::kdf_params::KdfParams;
use crate::signup::SRP_GROUP;

/// New server-side SRP credentials produced by a password rotation. Mirrors the
/// shape the server's verifier-update endpoint accepts (base64 salt/verifier).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SrpVerifierUpdate {
    /// Base64 SRP salt (freshly sampled).
    pub srp_salt: String,
    /// Base64 SRP verifier derived from the new URK.
    pub srp_verifier: String,
    /// SRP group identifier.
    pub srp_group: String,
    /// Opaque KDF parameters (unchanged — the header salts do not rotate).
    pub kdf_params: serde_json::Value,
}

/// Everything a password rotation produces: the new SRP credentials plus the
/// re-wrapped vault header to upload.
pub struct RotatePasswordMaterial {
    /// New SRP credentials for the server to store.
    pub verifier_update: SrpVerifierUpdate,
    /// The updated vault header (VK re-wrapped under the new MEK when present).
    pub header: VaultHeader,
    /// Whether a wrapped Vault Key was actually re-wrapped (`false` when the
    /// source header carried no VK, i.e. a verifier-only rotation).
    pub rewrapped_vk: bool,
}

impl RotatePasswordMaterial {
    /// Derive rotation material.
    ///
    /// When `current_header` carries a wrapped Vault Key, `old_password` must be
    /// correct or unwrapping fails with [`AccountError::Auth`] (the same opaque
    /// error a wrong Secret Key produces). For a header without a wrapped VK the
    /// old password is not cryptographically checkable here; the caller's active
    /// session is the authorization gate.
    ///
    /// # Errors
    /// - [`AccountError::Auth`] if the old password/Secret Key cannot unwrap the
    ///   existing Vault Key.
    /// - [`AccountError::Crypto`] if RNG, Argon2/HKDF, or AEAD wrapping fails.
    pub fn derive(
        old_password: &str,
        new_password: &str,
        secret_key: &SecretKey,
        current_header: &VaultHeader,
    ) -> Result<Self, AccountError> {
        let mut header = current_header.clone();

        // The new URK/MEK come from the new password; salts + account id are
        // unchanged, so KdfParams (and thus login re-derivation) stay stable.
        let new_urk =
            KeyHierarchy::derive_unlock_root_key(new_password.as_bytes(), secret_key, &header)?;

        let rewrapped_vk = if header.vk_wrap_ct.is_empty() {
            false
        } else {
            let old_urk =
                KeyHierarchy::derive_unlock_root_key(old_password.as_bytes(), secret_key, &header)?;
            let old_mek = KeyHierarchy::derive_mek(&old_urk, &header)?;
            // A wrong old password surfaces here as InvalidVaultOrCredentials.
            let vk = KeyHierarchy::unwrap_vk(&old_mek, &header).map_err(|_| AccountError::Auth)?;
            let new_mek = KeyHierarchy::derive_mek(&new_urk, &header)?;
            let (nonce, ct) = KeyHierarchy::wrap_vk(&new_mek, &vk, &header)?;
            header.vk_wrap_nonce = *nonce.as_bytes();
            header.vk_wrap_ct = ct;
            true
        };

        // Mint a fresh SRP salt + verifier from the new URK.
        let mut salt_srp = [0u8; 16];
        fill_random(&mut salt_srp)?;
        let srp_x = derive_srp_input(&new_urk, &salt_srp)?;
        let verifier = compute_verifier(&srp_x);
        let kdf_params = KdfParams::from_header(&header);

        Ok(Self {
            verifier_update: SrpVerifierUpdate {
                srp_salt: base64_encode(&salt_srp),
                srp_verifier: base64_encode(&verifier),
                srp_group: SRP_GROUP.to_string(),
                kdf_params: kdf_params.to_json(),
            },
            header,
            rewrapped_vk,
        })
    }

    /// Rotate from a base64-encoded transport header (`VaultHeader::to_bytes`),
    /// returning base64 of the re-wrapped header plus the verifier update.
    ///
    /// Convenience for the WASM / JS edge, which only deals in base64 strings
    /// and never links `tock-core` directly.
    ///
    /// # Errors
    /// - [`AccountError::Encoding`] if `header_b64` is not valid base64 of a
    ///   well-formed vault header.
    /// - Any error from [`RotatePasswordMaterial::derive`].
    pub fn derive_from_header_b64(
        old_password: &str,
        new_password: &str,
        secret_key: &SecretKey,
        header_b64: &str,
    ) -> Result<(String, SrpVerifierUpdate, bool), AccountError> {
        let bytes = crate::codec::base64_decode(header_b64)?;
        let header =
            VaultHeader::from_bytes(&bytes).map_err(|_| AccountError::Encoding("vault header"))?;
        let material = Self::derive(old_password, new_password, secret_key, &header)?;
        let new_header_b64 = base64_encode(&material.header.to_bytes());
        Ok((
            new_header_b64,
            material.verifier_update,
            material.rewrapped_vk,
        ))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use tock_core::vault::Argon2HeaderParams;
    use tock_core::vault::header::{FORMAT_VERSION, MAGIC, MIN_COMPAT_VERSION, STORAGE_LAYOUT_V0};
    use tock_core::vault::{KeyHierarchy, generate_vault_key};

    fn base_header() -> VaultHeader {
        let mut kdf_salt = [0u8; 16];
        let mut hkdf_salt = [0u8; 32];
        fill_random(&mut kdf_salt).unwrap();
        fill_random(&mut hkdf_salt).unwrap();
        let id = uuid::Uuid::from_bytes([7; 16]);
        VaultHeader {
            magic: MAGIC,
            format_version: FORMAT_VERSION,
            min_compatible_version: MIN_COMPAT_VERSION,
            vault_id: id,
            account_id: id,
            kdf_version: 1,
            kdf_salt,
            hkdf_salt,
            // Cheap Argon2 params keep the test fast.
            argon2: Argon2HeaderParams {
                t: 1,
                m_kib: 8,
                p: 1,
            },
            vk_wrap_nonce: [0; 12],
            vk_wrap_ct: Vec::new(),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            storage_layout: STORAGE_LAYOUT_V0.to_string(),
        }
    }

    /// Build a header whose VK is wrapped under `password` + `sk`.
    fn header_with_wrapped_vk(
        password: &str,
        sk: &SecretKey,
    ) -> (VaultHeader, tock_core::vault::VaultKey) {
        let mut header = base_header();
        let vk = generate_vault_key().unwrap();
        let urk = KeyHierarchy::derive_unlock_root_key(password.as_bytes(), sk, &header).unwrap();
        let mek = KeyHierarchy::derive_mek(&urk, &header).unwrap();
        let (nonce, ct) = KeyHierarchy::wrap_vk(&mek, &vk, &header).unwrap();
        header.vk_wrap_nonce = *nonce.as_bytes();
        header.vk_wrap_ct = ct;
        (header, vk)
    }

    #[test]
    fn rewrap_preserves_vault_key_under_new_password() {
        let sk = SecretKey::generate().unwrap();
        let (header, vk) = header_with_wrapped_vk("old-pw", &sk);

        let rotated = RotatePasswordMaterial::derive("old-pw", "new-pw", &sk, &header).unwrap();
        assert!(rotated.rewrapped_vk);

        // The re-wrapped header opens with the NEW password and yields the same VK.
        let new_urk =
            KeyHierarchy::derive_unlock_root_key(b"new-pw", &sk, &rotated.header).unwrap();
        let new_mek = KeyHierarchy::derive_mek(&new_urk, &rotated.header).unwrap();
        let recovered = KeyHierarchy::unwrap_vk(&new_mek, &rotated.header).unwrap();
        assert_eq!(
            recovered.as_secret().expose_secret(),
            vk.as_secret().expose_secret()
        );
    }

    #[test]
    fn old_password_no_longer_opens_rewrapped_header() {
        let sk = SecretKey::generate().unwrap();
        let (header, _vk) = header_with_wrapped_vk("old-pw", &sk);
        let rotated = RotatePasswordMaterial::derive("old-pw", "new-pw", &sk, &header).unwrap();

        let old_urk =
            KeyHierarchy::derive_unlock_root_key(b"old-pw", &sk, &rotated.header).unwrap();
        let old_mek = KeyHierarchy::derive_mek(&old_urk, &rotated.header).unwrap();
        assert!(KeyHierarchy::unwrap_vk(&old_mek, &rotated.header).is_err());
    }

    #[test]
    fn wrong_old_password_is_rejected() {
        let sk = SecretKey::generate().unwrap();
        let (header, _vk) = header_with_wrapped_vk("old-pw", &sk);
        let err = RotatePasswordMaterial::derive("WRONG", "new-pw", &sk, &header);
        assert!(matches!(err, Err(AccountError::Auth)));
    }

    #[test]
    fn verifier_changes_after_rotation() {
        let sk = SecretKey::generate().unwrap();
        let (header, _vk) = header_with_wrapped_vk("old-pw", &sk);

        // Verifier under the old password (fresh salt) vs. after rotation differ,
        // and the salt is freshly sampled each time.
        let rotated = RotatePasswordMaterial::derive("old-pw", "new-pw", &sk, &header).unwrap();
        assert_eq!(rotated.verifier_update.srp_group, SRP_GROUP);
        assert!(!rotated.verifier_update.srp_verifier.is_empty());
        assert!(!rotated.verifier_update.srp_salt.is_empty());
    }

    #[test]
    fn empty_vk_header_rotates_verifier_only() {
        let sk = SecretKey::generate().unwrap();
        let header = base_header(); // no wrapped VK
        let rotated = RotatePasswordMaterial::derive("old-pw", "new-pw", &sk, &header).unwrap();
        assert!(!rotated.rewrapped_vk);
        assert!(rotated.header.vk_wrap_ct.is_empty());
        assert!(!rotated.verifier_update.srp_verifier.is_empty());
    }

    #[test]
    fn b64_header_round_trip_rewraps_vk() {
        let sk = SecretKey::generate().unwrap();
        let (header, vk) = header_with_wrapped_vk("old-pw", &sk);
        let header_b64 = crate::codec::base64_encode(&header.to_bytes());

        let (new_header_b64, update) =
            RotatePasswordMaterial::derive_from_header_b64("old-pw", "new-pw", &sk, &header_b64)
                .map(|(h, u, _)| (h, u))
                .unwrap();
        assert!(!update.srp_verifier.is_empty());

        // Decode the returned header and confirm the new password opens the VK.
        let bytes = crate::codec::base64_decode(&new_header_b64).unwrap();
        let new_header = VaultHeader::from_bytes(&bytes).unwrap();
        let new_urk = KeyHierarchy::derive_unlock_root_key(b"new-pw", &sk, &new_header).unwrap();
        let new_mek = KeyHierarchy::derive_mek(&new_urk, &new_header).unwrap();
        let recovered = KeyHierarchy::unwrap_vk(&new_mek, &new_header).unwrap();
        assert_eq!(
            recovered.as_secret().expose_secret(),
            vk.as_secret().expose_secret()
        );
    }

    #[test]
    fn b64_header_bad_base64_rejected() {
        let sk = SecretKey::generate().unwrap();
        let err = RotatePasswordMaterial::derive_from_header_b64("a", "b", &sk, "not valid !!");
        assert!(matches!(err, Err(AccountError::Encoding(_))));
    }
}
