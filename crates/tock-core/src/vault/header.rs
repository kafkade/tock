//! Vault header — metadata that lives alongside the encrypted vault.
//!
//! The header has no I/O: it serializes to a `BTreeMap<&'static str,
//! Vec<u8>>` and the storage layer is responsible for persisting that
//! map (today: as rows in a `vault_meta` table; future: as a 256-byte
//! binary prelude when `SQLCipher` integration lands).
//!
//! ## Tamper resistance
//!
//! Every field that participates in key derivation is bound into the
//! AES-GCM AAD that wraps the Vault Key. An attacker who flips bits in
//! the header (e.g. weakens Argon2 parameters or swaps salts) will see
//! the same `InvalidVaultOrCredentials` error as someone who typed the
//! wrong password — they cannot distinguish.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::Error;

/// Four-byte magic identifier (`b"KAFD"`).
pub const MAGIC: [u8; 4] = *b"KAFD";

/// Current on-disk vault format version.
///
/// Bumped to `2` for the two-secret key derivation (2SKD, ADR-011):
/// the header now carries `account_id` and `kdf_version`, and the key
/// hierarchy roots in the Unlock Root Key (password + Secret Key)
/// instead of the password alone.
pub const FORMAT_VERSION: u16 = 2;

/// Lowest format version this build can open. Files with a higher
/// `format_version` than [`FORMAT_VERSION`] are refused.
///
/// 2SKD vaults are not backward-compatible with password-only `v1`
/// vaults; those must be re-initialized (pre-1.0, no automatic
/// migration). See [`VaultHeader::from_meta`].
pub const MIN_COMPAT_VERSION: u16 = 2;

/// Storage layout marker for this build.
///
/// Distinguishes the Phase 0 plain-SQLite-with-app-AEAD format from a
/// future `SQLCipher`-backed format. When `SQLCipher` integration lands, a
/// migration will need to handle this transition explicitly.
pub const STORAGE_LAYOUT_V0: &str = "sqlite-plain-app-aead-v0";

/// Argon2id parameters as stored in the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argon2HeaderParams {
    /// Iterations.
    pub t: u32,
    /// Memory cost in KiB.
    pub m_kib: u32,
    /// Parallelism.
    pub p: u32,
}

/// Vault header (metadata only — no I/O).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultHeader {
    /// Magic bytes — always [`MAGIC`].
    pub magic: [u8; 4],
    /// Format version of this vault (currently [`FORMAT_VERSION`]).
    pub format_version: u16,
    /// Lowest version of tock that can still open this vault.
    pub min_compatible_version: u16,
    /// Globally unique vault identifier (`UUIDv7`).
    pub vault_id: Uuid,
    /// Server-assigned account identifier this vault belongs to
    /// (`UUIDv7`). Bound into the 2SKD Secret-Key step and the wrap AAD.
    pub account_id: Uuid,
    /// Two-secret KDF version. Selects Argon2id parameters and the 2SKD
    /// `info` labels; bumping it is a forward-compatible re-wrap.
    pub kdf_version: u16,
    /// Salt for the password → MK Argon2id step (16 bytes).
    pub kdf_salt: [u8; 16],
    /// Salt for the MK → MEK HKDF step (32 bytes).
    pub hkdf_salt: [u8; 32],
    /// Argon2id parameters.
    pub argon2: Argon2HeaderParams,
    /// AES-GCM nonce used when wrapping VK with MEK (12 bytes).
    pub vk_wrap_nonce: [u8; 12],
    /// AES-GCM ciphertext (32 bytes VK + 16 bytes tag) of the wrapped VK.
    pub vk_wrap_ct: Vec<u8>,
    /// When the vault was created.
    pub created_at: OffsetDateTime,
    /// Identifier for the on-disk storage layout (see [`STORAGE_LAYOUT_V0`]).
    pub storage_layout: String,
}

impl VaultHeader {
    /// Build the canonical AAD used when wrapping/unwrapping VK with
    /// AES-256-GCM.
    ///
    /// The AAD covers every metadata field that participates in key
    /// derivation. Binding these into the AAD means any tampering of
    /// the persisted header invalidates the wrap and surfaces as
    /// [`Error::InvalidVaultOrCredentials`].
    ///
    /// Layout (all little-endian where applicable; length-prefixed for
    /// byte fields):
    ///
    /// ```text
    /// "tock-vault-header-v1"
    /// || magic[4]
    /// || format_version u16
    /// || min_compatible_version u16
    /// || vault_id[16]
    /// || account_id[16]
    /// || kdf_version u16
    /// || kdf_salt[16]
    /// || hkdf_salt[32]
    /// || argon2_t u32 || argon2_m_kib u32 || argon2_p u32
    /// || vk_wrap_nonce[12]
    /// || storage_layout: u16 len || bytes
    /// || created_at_unix_seconds i64 || nanos i32
    /// ```
    #[must_use]
    pub fn canonical_aad(&self) -> Vec<u8> {
        const DOMAIN: &[u8] = b"tock-vault-header-v1";
        let mut out = Vec::with_capacity(DOMAIN.len() + 160);
        out.extend_from_slice(DOMAIN);
        out.extend_from_slice(&self.magic);
        out.extend_from_slice(&self.format_version.to_le_bytes());
        out.extend_from_slice(&self.min_compatible_version.to_le_bytes());
        out.extend_from_slice(self.vault_id.as_bytes());
        out.extend_from_slice(self.account_id.as_bytes());
        out.extend_from_slice(&self.kdf_version.to_le_bytes());
        out.extend_from_slice(&self.kdf_salt);
        out.extend_from_slice(&self.hkdf_salt);
        out.extend_from_slice(&self.argon2.t.to_le_bytes());
        out.extend_from_slice(&self.argon2.m_kib.to_le_bytes());
        out.extend_from_slice(&self.argon2.p.to_le_bytes());
        out.extend_from_slice(&self.vk_wrap_nonce);
        let layout = self.storage_layout.as_bytes();
        let layout_len = u16::try_from(layout.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&layout_len.to_le_bytes());
        out.extend_from_slice(layout);
        out.extend_from_slice(&self.created_at.unix_timestamp().to_le_bytes());
        let nanos = i32::try_from(self.created_at.nanosecond()).unwrap_or(i32::MAX);
        out.extend_from_slice(&nanos.to_le_bytes());
        out
    }

    /// Serialize to a `BTreeMap<&'static str, Vec<u8>>` for the
    /// storage layer to persist.
    ///
    /// Keys are short ASCII strings; values are raw bytes (integers
    /// little-endian, UUIDs as 16 bytes, timestamps as ISO 8601).
    #[must_use]
    pub fn to_meta(&self) -> BTreeMap<&'static str, Vec<u8>> {
        let mut m = BTreeMap::new();
        m.insert("magic", self.magic.to_vec());
        m.insert("format_version", self.format_version.to_le_bytes().to_vec());
        m.insert(
            "min_compatible_version",
            self.min_compatible_version.to_le_bytes().to_vec(),
        );
        m.insert("vault_id", self.vault_id.as_bytes().to_vec());
        m.insert("account_id", self.account_id.as_bytes().to_vec());
        m.insert("kdf_version", self.kdf_version.to_le_bytes().to_vec());
        m.insert("kdf_salt", self.kdf_salt.to_vec());
        m.insert("hkdf_salt", self.hkdf_salt.to_vec());
        m.insert("argon2_t", self.argon2.t.to_le_bytes().to_vec());
        m.insert("argon2_m_kib", self.argon2.m_kib.to_le_bytes().to_vec());
        m.insert("argon2_p", self.argon2.p.to_le_bytes().to_vec());
        m.insert("vk_wrap_nonce", self.vk_wrap_nonce.to_vec());
        m.insert("vk_wrap_ct", self.vk_wrap_ct.clone());
        let ts = self
            .created_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        m.insert("created_at", ts.into_bytes());
        m.insert("storage_layout", self.storage_layout.as_bytes().to_vec());
        m
    }

    /// Deserialize from the same map shape produced by [`Self::to_meta`].
    ///
    /// # Errors
    /// - [`Error::MissingHeaderField`] if a required key is absent.
    /// - [`Error::InvalidEncoding`] if a value has the wrong length or
    ///   format.
    /// - [`Error::UnsupportedVaultVersion`] if `min_compatible_version`
    ///   exceeds [`FORMAT_VERSION`].
    pub fn from_meta(m: &BTreeMap<String, Vec<u8>>) -> Result<Self, Error> {
        fn get<'a>(
            m: &'a BTreeMap<String, Vec<u8>>,
            k: &'static str,
        ) -> Result<&'a Vec<u8>, Error> {
            m.get(k).ok_or(Error::MissingHeaderField(k))
        }
        fn u16_le(b: &[u8]) -> Result<u16, Error> {
            <[u8; 2]>::try_from(b)
                .map(u16::from_le_bytes)
                .map_err(|_| Error::InvalidEncoding)
        }
        fn u32_le(b: &[u8]) -> Result<u32, Error> {
            <[u8; 4]>::try_from(b)
                .map(u32::from_le_bytes)
                .map_err(|_| Error::InvalidEncoding)
        }
        fn arr<const N: usize>(b: &[u8]) -> Result<[u8; N], Error> {
            <[u8; N]>::try_from(b).map_err(|_| Error::InvalidEncoding)
        }

        let magic = arr::<4>(get(m, "magic")?)?;
        if magic != MAGIC {
            return Err(Error::InvalidEncoding);
        }
        let format_version = u16_le(get(m, "format_version")?)?;
        let min_compatible_version = u16_le(get(m, "min_compatible_version")?)?;
        if min_compatible_version > FORMAT_VERSION {
            return Err(Error::UnsupportedVaultVersion {
                required: min_compatible_version,
                supported: FORMAT_VERSION,
            });
        }
        // Legacy password-only vaults (v1) lack the 2SKD account binding
        // and cannot be opened — surface a clear re-init error rather
        // than a confusing missing-field failure.
        if format_version < MIN_COMPAT_VERSION {
            return Err(Error::VaultNeedsReinit {
                found: format_version,
            });
        }
        let vault_id_bytes = arr::<16>(get(m, "vault_id")?)?;
        let vault_id = Uuid::from_bytes(vault_id_bytes);
        let account_id_bytes = arr::<16>(get(m, "account_id")?)?;
        let account_id = Uuid::from_bytes(account_id_bytes);
        let kdf_version = u16_le(get(m, "kdf_version")?)?;
        let kdf_salt = arr::<16>(get(m, "kdf_salt")?)?;
        let hkdf_salt = arr::<32>(get(m, "hkdf_salt")?)?;
        let argon2 = Argon2HeaderParams {
            t: u32_le(get(m, "argon2_t")?)?,
            m_kib: u32_le(get(m, "argon2_m_kib")?)?,
            p: u32_le(get(m, "argon2_p")?)?,
        };
        let vk_wrap_nonce = arr::<12>(get(m, "vk_wrap_nonce")?)?;
        let vk_wrap_ct = get(m, "vk_wrap_ct")?.clone();
        let created_at_raw =
            std::str::from_utf8(get(m, "created_at")?).map_err(|_| Error::InvalidEncoding)?;
        let created_at = OffsetDateTime::parse(
            created_at_raw,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|_| Error::InvalidEncoding)?;
        let storage_layout = String::from_utf8(get(m, "storage_layout")?.clone())
            .map_err(|_| Error::InvalidEncoding)?;

        Ok(Self {
            magic,
            format_version,
            min_compatible_version,
            vault_id,
            account_id,
            kdf_version,
            kdf_salt,
            hkdf_salt,
            argon2,
            vk_wrap_nonce,
            vk_wrap_ct,
            created_at,
            storage_layout,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::missing_const_for_fn,
        clippy::panic
    )]

    use super::*;

    fn sample_header() -> VaultHeader {
        VaultHeader {
            magic: MAGIC,
            format_version: FORMAT_VERSION,
            min_compatible_version: MIN_COMPAT_VERSION,
            vault_id: Uuid::from_bytes([7; 16]),
            account_id: Uuid::from_bytes([8; 16]),
            kdf_version: 1,
            kdf_salt: [9; 16],
            hkdf_salt: [3; 32],
            argon2: Argon2HeaderParams {
                t: 3,
                m_kib: 65_536,
                p: 1,
            },
            vk_wrap_nonce: [5; 12],
            vk_wrap_ct: vec![0xAB; 48],
            created_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("ts"),
            storage_layout: STORAGE_LAYOUT_V0.to_string(),
        }
    }

    fn into_owned(m: BTreeMap<&'static str, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
        m.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    #[test]
    fn meta_roundtrip() {
        let h = sample_header();
        let parsed = VaultHeader::from_meta(&into_owned(h.to_meta())).expect("parse");
        assert_eq!(parsed, h);
    }

    #[test]
    fn missing_field_errors() {
        let h = sample_header();
        let mut owned = into_owned(h.to_meta());
        owned.remove("vault_id");
        match VaultHeader::from_meta(&owned) {
            Err(Error::MissingHeaderField(name)) => assert_eq!(name, "vault_id"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn bad_magic_rejected() {
        let h = sample_header();
        let mut owned = into_owned(h.to_meta());
        owned.insert("magic".to_string(), vec![0; 4]);
        assert!(matches!(
            VaultHeader::from_meta(&owned),
            Err(Error::InvalidEncoding)
        ));
    }

    #[test]
    fn future_version_rejected() {
        let mut h = sample_header();
        h.min_compatible_version = FORMAT_VERSION + 1;
        let owned = into_owned(h.to_meta());
        assert!(matches!(
            VaultHeader::from_meta(&owned),
            Err(Error::UnsupportedVaultVersion { .. })
        ));
    }

    #[test]
    fn canonical_aad_changes_when_any_field_changes() {
        let h = sample_header();
        let aad = h.canonical_aad();
        let mut h2 = h.clone();
        h2.kdf_salt[0] ^= 0x01;
        assert_ne!(aad, h2.canonical_aad());

        let mut h3 = h.clone();
        h3.argon2.t += 1;
        assert_ne!(aad, h3.canonical_aad());

        let mut h_acct = h.clone();
        h_acct.account_id = Uuid::from_bytes([99; 16]);
        assert_ne!(aad, h_acct.canonical_aad());

        let mut h_ver = h.clone();
        h_ver.kdf_version += 1;
        assert_ne!(aad, h_ver.canonical_aad());

        let mut h4 = h;
        h4.storage_layout.push('x');
        assert_ne!(h4.canonical_aad(), sample_header().canonical_aad());
    }

    #[test]
    fn legacy_v1_vault_needs_reinit() {
        let h = sample_header();
        let mut owned = into_owned(h.to_meta());
        // Simulate a legacy password-only vault: format/compat = 1.
        owned.insert("format_version".to_string(), 1_u16.to_le_bytes().to_vec());
        owned.insert(
            "min_compatible_version".to_string(),
            1_u16.to_le_bytes().to_vec(),
        );
        assert!(matches!(
            VaultHeader::from_meta(&owned),
            Err(Error::VaultNeedsReinit { found: 1 })
        ));
    }
}
