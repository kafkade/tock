//! Key derivation hierarchy: (password + Secret Key) → URK → MEK → VK →
//! `DK_kind` → IK.
//!
//! Architecture §5.1 and ADR-011 define the hierarchy; this module wraps
//! the primitives from `tock-crypto` to enforce the canonical info
//! strings and AAD construction. The root is the **Unlock Root Key
//! (URK)** from two-secret key derivation (2SKD): a stolen vault file is
//! uncrackable without the account Secret Key.

use tock_crypto::SecretBytes;
use tock_crypto::SecretKey;
use tock_crypto::aead::{self, Key as AeadKey, Nonce};
use tock_crypto::kdf::{Argon2Params, derive_unlock_root_key, hkdf_sha256_32};
use zeroize::Zeroizing;

use crate::Error;
use crate::vault::header::VaultHeader;

/// Domain-separated HKDF `info` string for deriving MEK from MK.
const INFO_MEK: &[u8] = b"tock/v1/mek";

/// HKDF `info` prefix for per-entity-kind domain keys (`DK_kind`).
const INFO_DK_PREFIX: &[u8] = b"tock/v1/item/";

/// HKDF `info` prefix for per-entity item keys (`IK`).
const INFO_IK_PREFIX: &[u8] = b"item/";

/// 256-bit Vault Key — unwraps to give access to the encrypted data.
pub struct VaultKey(SecretBytes<32>);

impl VaultKey {
    /// Wrap pre-existing 32 bytes as the Vault Key.
    #[must_use]
    pub const fn from_secret(bytes: SecretBytes<32>) -> Self {
        Self(bytes)
    }

    /// Borrow the underlying secret.
    #[must_use]
    pub const fn as_secret(&self) -> &SecretBytes<32> {
        &self.0
    }
}

impl core::fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("VaultKey<REDACTED>")
    }
}

/// Operations on the key hierarchy. Pure compute — no I/O.
pub struct KeyHierarchy;

impl KeyHierarchy {
    /// Derive the **Unlock Root Key (URK)** from the user password *and*
    /// the account [`SecretKey`] via two-secret key derivation (2SKD,
    /// ADR-011), using the salt, account id, KDF version, and Argon2id
    /// parameters stored in `header`.
    ///
    /// The URK replaces the password-only Master Key at the root of the
    /// hierarchy: `URK → MEK → VK → DK_kind → IK`.
    ///
    /// # Errors
    /// Returns [`Error::Crypto`] if Argon2 or HKDF fails (e.g. memory
    /// allocation).
    pub fn derive_unlock_root_key(
        password: &[u8],
        secret_key: &SecretKey,
        header: &VaultHeader,
    ) -> Result<SecretBytes<32>, Error> {
        let params = Argon2Params::new(header.argon2.t, header.argon2.m_kib, header.argon2.p)
            .map_err(Error::from)?;
        derive_unlock_root_key(
            password,
            secret_key.expose_secret(),
            &header.kdf_salt,
            header.account_id.as_bytes(),
            header.kdf_version,
            params,
        )
        .map_err(Error::from)
    }

    /// Derive the Master Encryption Key from the URK using HKDF-SHA256
    /// with the header's `hkdf_salt`.
    ///
    /// # Errors
    /// Returns [`Error::Crypto`] only if HKDF rejects its inputs (should
    /// not happen for the 32-byte output).
    pub fn derive_mek(
        urk: &SecretBytes<32>,
        header: &VaultHeader,
    ) -> Result<SecretBytes<32>, Error> {
        hkdf_sha256_32(urk.expose_secret(), &header.hkdf_salt, INFO_MEK).map_err(Error::from)
    }

    /// Wrap the Vault Key under the MEK, binding the wrap to the
    /// header's canonical AAD so any header tampering invalidates the
    /// resulting ciphertext.
    ///
    /// Returns `(nonce, ciphertext)`. The caller stores both in the
    /// header (`vk_wrap_nonce` and `vk_wrap_ct`) before persisting.
    /// Note that the header passed in must already contain the same
    /// `vk_wrap_nonce` that this call returns (the AAD includes the
    /// nonce); pass the header *with* `vk_wrap_nonce` populated and
    /// re-call if rolling.
    ///
    /// # Errors
    /// Returns [`Error::Crypto`] if RNG or AEAD encryption fails.
    pub fn wrap_vk(
        mek: &SecretBytes<32>,
        vk: &VaultKey,
        header: &VaultHeader,
    ) -> Result<(Nonce, Vec<u8>), Error> {
        let nonce = Nonce::try_random().map_err(Error::from)?;
        let mut header_for_aad = header.clone();
        header_for_aad.vk_wrap_nonce = *nonce.as_bytes();
        let key = AeadKey::from_secret(mek.clone_secret());
        let aad = header_for_aad.canonical_aad();
        let ct = aead::seal(&key, &nonce, &aad, vk.0.expose_secret()).map_err(Error::from)?;
        Ok((nonce, ct))
    }

    /// Unwrap the Vault Key from the header. Returns
    /// [`Error::InvalidVaultOrCredentials`] for *any* failure — wrong
    /// password, tampered header, or truncated ciphertext.
    ///
    /// # Errors
    /// See above. The variant is intentionally identical so the caller
    /// cannot distinguish the failure cause.
    pub fn unwrap_vk(mek: &SecretBytes<32>, header: &VaultHeader) -> Result<VaultKey, Error> {
        let key = AeadKey::from_secret(mek.clone_secret());
        let nonce = Nonce::from_bytes(header.vk_wrap_nonce);
        let aad = header.canonical_aad();
        let pt: Zeroizing<Vec<u8>> = aead::open(&key, &nonce, &aad, &header.vk_wrap_ct)
            .map_err(|_| Error::InvalidVaultOrCredentials)?;
        let bytes: [u8; 32] = pt
            .as_slice()
            .try_into()
            .map_err(|_| Error::InvalidVaultOrCredentials)?;
        Ok(VaultKey(SecretBytes::new(bytes)))
    }

    /// Derive a domain key for a given entity kind.
    ///
    /// # Errors
    /// Returns [`Error::Crypto`] only on internal HKDF failure.
    pub fn derive_domain_key(vk: &VaultKey, entity_kind: &str) -> Result<SecretBytes<32>, Error> {
        let mut info = Vec::with_capacity(INFO_DK_PREFIX.len() + entity_kind.len());
        info.extend_from_slice(INFO_DK_PREFIX);
        info.extend_from_slice(entity_kind.as_bytes());
        hkdf_sha256_32(vk.0.expose_secret(), &[], &info).map_err(Error::from)
    }

    /// Derive a per-item key from a domain key and a 16-byte entity ID.
    ///
    /// # Errors
    /// Returns [`Error::Crypto`] only on internal HKDF failure.
    pub fn derive_item_key(
        dk: &SecretBytes<32>,
        entity_id: &[u8; 16],
    ) -> Result<SecretBytes<32>, Error> {
        let mut info = Vec::with_capacity(INFO_IK_PREFIX.len() + entity_id.len());
        info.extend_from_slice(INFO_IK_PREFIX);
        info.extend_from_slice(entity_id);
        hkdf_sha256_32(dk.expose_secret(), &[], &info).map_err(Error::from)
    }
}

/// Build a fresh, randomly-generated Vault Key.
///
/// # Errors
/// Returns [`Error::Crypto`] on RNG failure.
pub fn generate_vault_key() -> Result<VaultKey, Error> {
    Ok(VaultKey(SecretBytes::try_random().map_err(Error::from)?))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::vault::header::{
        Argon2HeaderParams, FORMAT_VERSION, MAGIC, MIN_COMPAT_VERSION, STORAGE_LAYOUT_V0,
    };
    use time::OffsetDateTime;
    use uuid::Uuid;

    const fn fast_argon_params() -> Argon2HeaderParams {
        Argon2HeaderParams {
            t: 1,
            m_kib: 8,
            p: 1,
        }
    }

    fn test_secret_key() -> SecretKey {
        SecretKey::from_bytes([0x5A; 16])
    }

    fn header_skeleton(nonce: [u8; 12], ct: Vec<u8>) -> VaultHeader {
        VaultHeader {
            magic: MAGIC,
            format_version: FORMAT_VERSION,
            min_compatible_version: MIN_COMPAT_VERSION,
            vault_id: Uuid::from_bytes([1; 16]),
            account_id: Uuid::from_bytes([4; 16]),
            kdf_version: 1,
            kdf_salt: [2; 16],
            hkdf_salt: [3; 32],
            argon2: fast_argon_params(),
            vk_wrap_nonce: nonce,
            vk_wrap_ct: ct,
            created_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("ts"),
            storage_layout: STORAGE_LAYOUT_V0.to_string(),
        }
    }

    #[test]
    fn wrap_then_unwrap_roundtrips() {
        let header_skel = header_skeleton([0; 12], vec![]);
        let sk = test_secret_key();
        let urk = KeyHierarchy::derive_unlock_root_key(b"hunter2", &sk, &header_skel).expect("urk");
        let mek = KeyHierarchy::derive_mek(&urk, &header_skel).expect("mek");
        let vk = generate_vault_key().expect("vk");

        let (nonce, ct) = KeyHierarchy::wrap_vk(&mek, &vk, &header_skel).expect("wrap");
        let final_header = header_skeleton(*nonce.as_bytes(), ct);

        let recovered = KeyHierarchy::unwrap_vk(&mek, &final_header).expect("unwrap");
        assert_eq!(recovered.as_secret(), vk.as_secret());
    }

    #[test]
    #[allow(clippy::similar_names)]
    fn wrong_password_yields_invalid_credentials() {
        let header_skel = header_skeleton([0; 12], vec![]);
        let sk = test_secret_key();
        let urk = KeyHierarchy::derive_unlock_root_key(b"hunter2", &sk, &header_skel).expect("urk");
        let mek = KeyHierarchy::derive_mek(&urk, &header_skel).expect("mek");
        let vk = generate_vault_key().expect("vk");
        let (nonce, ct) = KeyHierarchy::wrap_vk(&mek, &vk, &header_skel).expect("wrap");
        let final_header = header_skeleton(*nonce.as_bytes(), ct);

        let wrong_urk =
            KeyHierarchy::derive_unlock_root_key(b"wrong", &sk, &final_header).expect("urk-wrong");
        let wrong_mek = KeyHierarchy::derive_mek(&wrong_urk, &final_header).expect("mek-wrong");
        assert!(matches!(
            KeyHierarchy::unwrap_vk(&wrong_mek, &final_header),
            Err(Error::InvalidVaultOrCredentials)
        ));
    }

    #[test]
    #[allow(clippy::similar_names)]
    fn wrong_secret_key_yields_invalid_credentials() {
        let header_skel = header_skeleton([0; 12], vec![]);
        let sk = test_secret_key();
        let urk = KeyHierarchy::derive_unlock_root_key(b"hunter2", &sk, &header_skel).expect("urk");
        let mek = KeyHierarchy::derive_mek(&urk, &header_skel).expect("mek");
        let vk = generate_vault_key().expect("vk");
        let (nonce, ct) = KeyHierarchy::wrap_vk(&mek, &vk, &header_skel).expect("wrap");
        let final_header = header_skeleton(*nonce.as_bytes(), ct);

        // Correct password, wrong Secret Key → cannot unwrap VK.
        let wrong_sk = SecretKey::from_bytes([0xA5; 16]);
        let wrong_urk = KeyHierarchy::derive_unlock_root_key(b"hunter2", &wrong_sk, &final_header)
            .expect("urk-wrong");
        let wrong_mek = KeyHierarchy::derive_mek(&wrong_urk, &final_header).expect("mek-wrong");
        assert!(matches!(
            KeyHierarchy::unwrap_vk(&wrong_mek, &final_header),
            Err(Error::InvalidVaultOrCredentials)
        ));
    }

    #[test]
    fn tampered_header_yields_invalid_credentials() {
        let header_skel = header_skeleton([0; 12], vec![]);
        let sk = test_secret_key();
        let urk = KeyHierarchy::derive_unlock_root_key(b"hunter2", &sk, &header_skel).expect("urk");
        let mek = KeyHierarchy::derive_mek(&urk, &header_skel).expect("mek");
        let vk = generate_vault_key().expect("vk");
        let (nonce, ct) = KeyHierarchy::wrap_vk(&mek, &vk, &header_skel).expect("wrap");
        let mut final_header = header_skeleton(*nonce.as_bytes(), ct);

        // Flip a bit in something that participates in AAD (the salt).
        final_header.kdf_salt[0] ^= 0x01;
        assert!(matches!(
            KeyHierarchy::unwrap_vk(&mek, &final_header),
            Err(Error::InvalidVaultOrCredentials)
        ));
    }

    #[test]
    fn item_keys_differ_per_entity_id() {
        let vk = generate_vault_key().expect("vk");
        let dk_task = KeyHierarchy::derive_domain_key(&vk, "task").expect("dk");
        let ik_a = KeyHierarchy::derive_item_key(&dk_task, &[1; 16]).expect("ik");
        let ik_b = KeyHierarchy::derive_item_key(&dk_task, &[2; 16]).expect("ik");
        assert_ne!(ik_a, ik_b);
    }

    #[test]
    fn domain_keys_differ_per_kind() {
        let vk = generate_vault_key().expect("vk");
        let dk_task = KeyHierarchy::derive_domain_key(&vk, "task").expect("dk");
        let dk_habit = KeyHierarchy::derive_domain_key(&vk, "habit").expect("dk");
        assert_ne!(dk_task, dk_habit);
    }
}
