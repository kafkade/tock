//! Recovery key and password/key rotation.
//!
//! ## Recovery key (architecture §5.4)
//!
//! A recovery key is 256 bits of CSPRNG entropy encoded as a
//! **Crockford Base32** string (52 characters, grouped for
//! readability). The recovery key provides an alternate path to
//! unwrap the Vault Key without the password.
//!
//! ```text
//! CSPRNG(256 bits)
//!   → Crockford Base32 encode → 52 chars (+ 1 check char = 53)
//!   → HKDF(ikm=decoded_bytes, salt=vault_id, info="tock/v1/recovery")
//!   → RK (32 bytes)
//!   → AES-256-GCM(RK, nonce, VK, aad=header_prefix)
//!   → vk_recover_ct
//! ```
//!
//! ## Password rotation
//!
//! Re-derives MK and MEK from the new password and re-wraps VK.
//! Does not change VK itself — all item keys remain valid.
//!
//! ## Full VK rotation (plan only)
//!
//! Generates a new VK. All items encrypted under the old VK must be
//! re-encrypted, so this returns a [`RotationPlan`] that the caller
//! executes transactionally.

use tock_core::vault::header::VaultHeader;
use tock_core::vault::keys::{KeyHierarchy, VaultKey, generate_vault_key};
use tock_crypto::SecretBytes;
use tock_crypto::aead::{self, Key as AeadKey, Nonce};
use tock_crypto::kdf::hkdf_sha256_32;
use uuid::Uuid;

use crate::Error;

/// HKDF info for deriving the recovery key from the raw entropy.
const RECOVERY_KEY_INFO: &[u8] = b"tock/v1/recovery";

/// AAD for recovery-key VK wrapping.
const RECOVERY_WRAP_AAD: &[u8] = b"tock-recovery-wrap-v1";

// ── Crockford Base32 ─────────────────────────────────────────────────

/// Crockford Base32 alphabet (excludes I, L, O, U to avoid confusion).
const CROCKFORD_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Encode bytes to Crockford Base32 string.
#[must_use]
pub fn crockford_encode(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    // 5 bits per character. Total bits = data.len() * 8.
    let total_bits = data.len() * 8;
    let num_chars = total_bits.div_ceil(5);
    let mut result = Vec::with_capacity(num_chars);
    let mut bit_buffer: u64 = 0;
    let mut bits_in_buffer: u32 = 0;

    for &byte in data {
        bit_buffer = (bit_buffer << 8) | u64::from(byte);
        bits_in_buffer += 8;
        while bits_in_buffer >= 5 {
            bits_in_buffer -= 5;
            let idx = ((bit_buffer >> bits_in_buffer) & 0x1F) as usize;
            result.push(CROCKFORD_ALPHABET[idx]);
        }
    }
    // Remaining bits (< 5), left-pad with zeros.
    if bits_in_buffer > 0 {
        let idx = ((bit_buffer << (5 - bits_in_buffer)) & 0x1F) as usize;
        result.push(CROCKFORD_ALPHABET[idx]);
    }
    String::from_utf8(result).unwrap_or_default()
}

/// Decode a Crockford Base32 string to bytes.
///
/// Handles normalization: lowercase → uppercase, `O` → `0`, `I`/`L` → `1`.
/// Skips dashes and spaces.
///
/// # Errors
/// Returns [`Error::WireFormat`] on invalid characters.
pub fn crockford_decode(input: &str, expected_bytes: usize) -> Result<Vec<u8>, Error> {
    let mut values = Vec::new();
    for ch in input.chars() {
        let ch = match ch {
            '-' | ' ' => continue,
            'a'..='z' => ch.to_ascii_uppercase(),
            other => other,
        };
        // Normalize ambiguous characters.
        let ch = match ch {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        let Some(val) = CROCKFORD_ALPHABET.iter().position(|&c| c == ch as u8) else {
            return Err(Error::WireFormat("invalid Crockford Base32 character"));
        };
        values.push(u8::try_from(val).unwrap_or(0));
    }

    // Reconstruct bytes from 5-bit values.
    let mut result = Vec::with_capacity(expected_bytes);
    let mut bit_buffer: u64 = 0;
    let mut bits_in_buffer: u32 = 0;
    for val in values {
        bit_buffer = (bit_buffer << 5) | u64::from(val);
        bits_in_buffer += 5;
        while bits_in_buffer >= 8 {
            bits_in_buffer -= 8;
            result.push(((bit_buffer >> bits_in_buffer) & 0xFF) as u8);
        }
    }
    // Truncate to expected length (padding bits are discarded).
    result.truncate(expected_bytes);
    if result.len() != expected_bytes {
        return Err(Error::WireFormat("recovery code too short"));
    }
    Ok(result)
}

/// Format a Crockford Base32 string with dashes for readability.
///
/// Groups of 4 characters separated by dashes:
/// `XXXX-XXXX-XXXX-...`
#[must_use]
pub fn format_recovery_code(code: &str) -> String {
    code.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("-")
}

// ── Recovery key ─────────────────────────────────────────────────────

/// A recovery key: the raw entropy and its Crockford Base32 encoding.
pub struct RecoveryKey {
    /// The derived 32-byte key (from HKDF over the raw entropy).
    key: SecretBytes<32>,
}

impl RecoveryKey {
    /// Borrow the derived key.
    #[must_use]
    pub const fn as_secret(&self) -> &SecretBytes<32> {
        &self.key
    }
}

impl core::fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("RecoveryKey<REDACTED>")
    }
}

/// Generate a fresh recovery key.
///
/// Returns the Crockford Base32 code (for the user to write down) and
/// the derived recovery key (for wrapping VK).
///
/// # Errors
/// Returns [`Error::Crypto`] on RNG failure.
pub fn generate_recovery_key(vault_id: Uuid) -> Result<(String, RecoveryKey), Error> {
    let mut entropy = [0_u8; 32];
    tock_crypto::random::fill_random(&mut entropy)?;
    let code = crockford_encode(&entropy);
    let formatted = format_recovery_code(&code);
    let key = derive_recovery_key_from_bytes(&entropy, vault_id)?;
    // Zero the raw entropy — the user has the code.
    zeroize_bytes(&mut entropy);
    Ok((formatted, key))
}

/// Derive a recovery key from a Crockford Base32 code string.
///
/// # Errors
/// Returns [`Error::WireFormat`] on invalid code or [`Error::Crypto`]
/// on HKDF failure.
pub fn derive_recovery_key(code: &str, vault_id: Uuid) -> Result<RecoveryKey, Error> {
    let bytes = crockford_decode(code, 32)?;
    derive_recovery_key_from_bytes(&bytes, vault_id)
}

fn derive_recovery_key_from_bytes(entropy: &[u8], vault_id: Uuid) -> Result<RecoveryKey, Error> {
    let derived = hkdf_sha256_32(entropy, vault_id.as_bytes(), RECOVERY_KEY_INFO)?;
    Ok(RecoveryKey { key: derived })
}

/// Wrap the vault key under a recovery key.
///
/// Returns `(nonce, ciphertext)` for storage in the vault header.
///
/// # Errors
/// Returns [`Error::Crypto`] on AEAD or RNG failure.
pub fn wrap_vk_with_recovery(
    rk: &RecoveryKey,
    vault_key: &VaultKey,
) -> Result<([u8; 12], Vec<u8>), Error> {
    let aead_key = AeadKey::from_secret(rk.key.clone_secret());
    let nonce = Nonce::try_random()?;
    let ct = aead::seal(
        &aead_key,
        &nonce,
        RECOVERY_WRAP_AAD,
        vault_key.as_secret().expose_secret(),
    )?;
    Ok((*nonce.as_bytes(), ct))
}

/// Unwrap the vault key using a recovery key.
///
/// # Errors
/// Returns [`Error::Crypto`] on AEAD failure (wrong recovery key,
/// tampered ciphertext, etc.).
pub fn unwrap_vk_from_recovery(
    rk: &RecoveryKey,
    nonce: &[u8; 12],
    ciphertext: &[u8],
) -> Result<VaultKey, Error> {
    let aead_key = AeadKey::from_secret(rk.key.clone_secret());
    let nonce = Nonce::from_bytes(*nonce);
    let pt = aead::open(&aead_key, &nonce, RECOVERY_WRAP_AAD, ciphertext)?;
    let bytes: [u8; 32] = pt
        .as_slice()
        .try_into()
        .map_err(|_| Error::Crypto(tock_crypto::Error::InvalidEncoding))?;
    Ok(VaultKey::from_secret(SecretBytes::new(bytes)))
}

// ── Password rotation ────────────────────────────────────────────────

/// Rotate the vault password.
///
/// Re-derives MK and MEK from the new password and re-wraps VK in the
/// header. Does **not** change VK — all item keys remain valid.
///
/// Returns the updated header with new `kdf_salt`, `hkdf_salt`,
/// `vk_wrap_nonce`, and `vk_wrap_ct`.
///
/// # Errors
/// - [`Error::Crypto`] on KDF / AEAD failure.
/// - [`Error::Core`] if the old password fails to unwrap VK.
pub fn rotate_password(
    #[allow(clippy::similar_names)] old_password: &[u8],
    new_password: &[u8],
    header: &VaultHeader,
) -> Result<VaultHeader, Error> {
    // Unwrap VK with old password.
    let old_master = KeyHierarchy::derive_master_key(old_password, header)?;
    let old_enc = KeyHierarchy::derive_mek(&old_master, header)?;
    let vk = KeyHierarchy::unwrap_vk(&old_enc, header)?;

    // Generate new salts.
    let mut kdf_salt = [0_u8; 16];
    let mut hkdf_salt = [0_u8; 32];
    tock_crypto::random::fill_random(&mut kdf_salt)?;
    tock_crypto::random::fill_random(&mut hkdf_salt)?;

    // Build new header skeleton with new salts.
    let mut new_header = header.clone();
    new_header.kdf_salt = kdf_salt;
    new_header.hkdf_salt = hkdf_salt;

    // Derive new MK, MEK and re-wrap VK.
    let new_master = KeyHierarchy::derive_master_key(new_password, &new_header)?;
    let new_enc = KeyHierarchy::derive_mek(&new_master, &new_header)?;
    let (nonce, ct) = KeyHierarchy::wrap_vk(&new_enc, &vk, &new_header)?;
    new_header.vk_wrap_nonce = *nonce.as_bytes();
    new_header.vk_wrap_ct = ct;

    Ok(new_header)
}

// ── Full VK rotation (plan only) ─────────────────────────────────────

/// An entity that needs re-encryption during VK rotation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RotationItem {
    /// Entity kind (e.g. "task", "habit").
    pub entity_kind: String,
    /// Entity UUID.
    pub entity_id: Uuid,
}

/// A plan for full VK rotation.
///
/// The caller must re-encrypt each item in [`items`] under keys
/// derived from the new VK, then atomically commit the new header
/// and re-encrypted items. This struct does **not** perform the
/// rotation — it only describes what needs to happen.
#[derive(Debug)]
pub struct RotationPlan {
    /// The new vault key.
    pub new_vault_key: VaultKey,
    /// Items that must be re-encrypted.
    pub items: Vec<RotationItem>,
}

/// Plan a full VK rotation.
///
/// Generates a new VK and returns the plan. The caller provides the
/// list of all entity (kind, id) pairs that exist in the vault.
///
/// # Errors
/// Returns [`Error::Crypto`] on RNG failure.
pub fn plan_vault_key_rotation(entities: Vec<RotationItem>) -> Result<RotationPlan, Error> {
    let new_vk = generate_vault_key()?;
    Ok(RotationPlan {
        new_vault_key: new_vk,
        items: entities,
    })
}

fn zeroize_bytes(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        *b = 0;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use time::OffsetDateTime;
    use tock_core::vault::generate_vault_key;
    use tock_core::vault::header::{
        Argon2HeaderParams, FORMAT_VERSION, MAGIC, MIN_COMPAT_VERSION, STORAGE_LAYOUT_V0,
    };

    const fn fast_argon() -> Argon2HeaderParams {
        Argon2HeaderParams {
            t: 1,
            m_kib: 8,
            p: 1,
        }
    }

    fn test_header() -> VaultHeader {
        VaultHeader {
            magic: MAGIC,
            format_version: FORMAT_VERSION,
            min_compatible_version: MIN_COMPAT_VERSION,
            vault_id: Uuid::from_bytes([1; 16]),
            kdf_salt: [2; 16],
            hkdf_salt: [3; 32],
            argon2: fast_argon(),
            vk_wrap_nonce: [0; 12],
            vk_wrap_ct: Vec::new(),
            created_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("ts"),
            storage_layout: STORAGE_LAYOUT_V0.to_string(),
        }
    }

    fn header_with_wrapped_vk(password: &[u8]) -> (VaultHeader, VaultKey) {
        let header_skel = test_header();
        let mk = KeyHierarchy::derive_master_key(password, &header_skel).expect("mk");
        let mek = KeyHierarchy::derive_mek(&mk, &header_skel).expect("mek");
        let vk = generate_vault_key().expect("vk");
        let (nonce, ct) = KeyHierarchy::wrap_vk(&mek, &vk, &header_skel).expect("wrap");
        let header = VaultHeader {
            vk_wrap_nonce: *nonce.as_bytes(),
            vk_wrap_ct: ct,
            ..header_skel
        };
        (header, vk)
    }

    // ── Crockford Base32 ─────────────────────────────────────────────

    #[test]
    fn crockford_roundtrip() {
        let data = [0xDE, 0xAD, 0xBE, 0xEF, 0x42];
        let encoded = crockford_encode(&data);
        let decoded = crockford_decode(&encoded, data.len()).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn crockford_roundtrip_32_bytes() {
        #[allow(clippy::cast_possible_truncation)]
        let data: [u8; 32] = core::array::from_fn(|i| i as u8);
        let encoded = crockford_encode(&data);
        assert_eq!(encoded.len(), 52); // 256 bits / 5 = 51.2 → 52 chars
        let decoded = crockford_decode(&encoded, 32).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn crockford_handles_dashes_and_spaces() {
        let data = [0xFF; 4];
        let encoded = crockford_encode(&data);
        let formatted = format_recovery_code(&encoded);
        assert!(formatted.contains('-'));
        let decoded = crockford_decode(&formatted, 4).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn crockford_normalizes_ambiguous_chars() {
        // 'O' → '0', 'I' → '1', 'L' → '1', lowercase → uppercase
        let encoded = crockford_encode(&[0]);
        let lowered = encoded.to_lowercase();
        let decoded = crockford_decode(&lowered, 1).expect("decode");
        assert_eq!(decoded, [0]);
    }

    #[test]
    fn crockford_rejects_invalid_char() {
        assert!(crockford_decode("HELLO!", 3).is_err());
    }

    #[test]
    fn format_recovery_code_groups_by_four() {
        let code = "ABCDEFGHMNPQ";
        let formatted = format_recovery_code(code);
        assert_eq!(formatted, "ABCD-EFGH-MNPQ");
    }

    // ── Recovery key ─────────────────────────────────────────────────

    #[test]
    fn recovery_key_generate_and_derive_roundtrip() {
        let vault_id = Uuid::from_bytes([42; 16]);
        let (code, rk) = generate_recovery_key(vault_id).expect("gen");

        // Derive from the code.
        let rk2 = derive_recovery_key(&code, vault_id).expect("derive");
        assert_eq!(rk.key, rk2.key);
    }

    #[test]
    fn recovery_key_wrap_unwrap_vk() {
        let vault_id = Uuid::from_bytes([42; 16]);
        let vk = generate_vault_key().expect("vk");
        let (_code, rk) = generate_recovery_key(vault_id).expect("gen");

        let (nonce, ct) = wrap_vk_with_recovery(&rk, &vk).expect("wrap");
        let recovered = unwrap_vk_from_recovery(&rk, &nonce, &ct).expect("unwrap");
        assert_eq!(
            recovered.as_secret().expose_secret(),
            vk.as_secret().expose_secret()
        );
    }

    #[test]
    fn recovery_key_wrong_key_fails() {
        let vault_id = Uuid::from_bytes([42; 16]);
        let vk = generate_vault_key().expect("vk");
        let (_code, rk) = generate_recovery_key(vault_id).expect("gen");
        let (nonce, ct) = wrap_vk_with_recovery(&rk, &vk).expect("wrap");

        // Different recovery key.
        let (_code2, rk2) = generate_recovery_key(vault_id).expect("gen2");
        assert!(unwrap_vk_from_recovery(&rk2, &nonce, &ct).is_err());
    }

    // ── Password rotation ────────────────────────────────────────────

    #[test]
    #[allow(clippy::similar_names)]
    fn password_rotation_roundtrip() {
        let (header, original_vk) = header_with_wrapped_vk(b"old-password");

        // Rotate.
        let new_header =
            rotate_password(b"old-password", b"new-password", &header).expect("rotate");

        // Old password should fail.
        let old_master = KeyHierarchy::derive_master_key(b"old-password", &new_header).expect("mk");
        let old_enc = KeyHierarchy::derive_mek(&old_master, &new_header).expect("mek");
        assert!(KeyHierarchy::unwrap_vk(&old_enc, &new_header).is_err());

        // New password should succeed and recover same VK.
        let new_master = KeyHierarchy::derive_master_key(b"new-password", &new_header).expect("mk");
        let new_enc = KeyHierarchy::derive_mek(&new_master, &new_header).expect("mek");
        let recovered = KeyHierarchy::unwrap_vk(&new_enc, &new_header).expect("unwrap");
        assert_eq!(
            recovered.as_secret().expose_secret(),
            original_vk.as_secret().expose_secret()
        );
    }

    #[test]
    fn password_rotation_wrong_old_password_fails() {
        let (header, _vk) = header_with_wrapped_vk(b"correct");
        assert!(rotate_password(b"wrong", b"new", &header).is_err());
    }

    // ── VK rotation plan ─────────────────────────────────────────────

    #[test]
    fn rotation_plan_lists_all_entities() {
        let items = vec![
            RotationItem {
                entity_kind: "task".into(),
                entity_id: Uuid::from_bytes([1; 16]),
            },
            RotationItem {
                entity_kind: "habit".into(),
                entity_id: Uuid::from_bytes([2; 16]),
            },
        ];
        let plan = plan_vault_key_rotation(items.clone()).expect("plan");
        assert_eq!(plan.items, items);
        // New VK should differ from any fixed value.
        assert_ne!(plan.new_vault_key.as_secret().expose_secret(), &[0; 32]);
    }
}
