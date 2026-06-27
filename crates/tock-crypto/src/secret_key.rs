//! Account **Secret Key** — the high-entropy "something you have" factor
//! in tock's two-secret key derivation (2SKD, see [`crate::kdf`] and
//! ADR-011).
//!
//! The Secret Key is 128 bits of CSPRNG entropy, generated client-side
//! at account creation and **never transmitted to the server or written
//! to the vault file**. It is surfaced to the user only through the
//! Emergency-Kit encoding ([`SecretKey::to_emergency_kit`]):
//!
//! ```text
//! A4-<ACCOUNTID>-<G1>-<G2>-<G3>-<G4>-<G5>-<G6>-<CK>
//! │   │           └─────── 128-bit secret, Crockford Base32 ──────┘  │
//! │   └ account id (Crockford Base32 of the 16-byte account UUID)     │
//! └ format/version tag ("A4"; bump on KDF change)                     └ 4-char checksum
//! ```
//!
//! The checksum is a truncated SHA-256 over the version tag, account id,
//! and secret, so a mistyped key is rejected before any Argon2id work.

use core::fmt;

use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::Error;
use crate::base32;
use crate::secret::SecretBytes;

/// Length of the raw Secret Key in bytes (128 bits).
pub const SECRET_KEY_BYTES: usize = 16;

/// Length of the account id in bytes (a UUID, handled here as raw bytes
/// so this crate stays free of a `uuid` dependency).
pub const ACCOUNT_ID_BYTES: usize = 16;

/// Format/version tag prefixing the Emergency-Kit encoding.
const VERSION_TAG: &str = "A4";

/// Crockford characters in the encoded account id (16 bytes → 26 chars).
const ACCOUNT_CHARS: usize = 26;

/// Crockford characters in the encoded secret (16 bytes → 26 chars).
const SECRET_CHARS: usize = 26;

/// Crockford characters in the checksum group.
const CHECKSUM_CHARS: usize = 4;

/// Group size used when formatting the secret for transcription.
const SECRET_GROUP: usize = 5;

/// A 128-bit account Secret Key. Zeroized on drop; never `Debug`-printed
/// in the clear and never serialized.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecretKey(SecretBytes<SECRET_KEY_BYTES>);

impl SecretKey {
    /// Generate a fresh Secret Key from the OS CSPRNG.
    ///
    /// # Errors
    /// Returns [`Error::Rng`] if the OS RNG fails.
    pub fn generate() -> Result<Self, Error> {
        Ok(Self(SecretBytes::try_random()?))
    }

    /// Wrap pre-existing 16 bytes as a Secret Key.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; SECRET_KEY_BYTES]) -> Self {
        Self(SecretBytes::new(bytes))
    }

    /// Borrow the raw secret bytes.
    ///
    /// **Hazmat.** Only call at the boundary where the raw bytes feed a
    /// primitive (e.g. the 2SKD HKDF step).
    #[must_use]
    pub const fn expose_secret(&self) -> &[u8; SECRET_KEY_BYTES] {
        self.0.expose_secret()
    }

    /// Deliberate, greppable copy. Use sparingly.
    #[must_use]
    pub const fn clone_secret(&self) -> Self {
        Self(self.0.clone_secret())
    }

    /// Encode as the user-facing Emergency-Kit string, binding the
    /// `account_id` and a transcription checksum.
    #[must_use]
    pub fn to_emergency_kit(&self, account_id: &[u8; ACCOUNT_ID_BYTES]) -> String {
        let account = base32::encode(account_id);
        let secret = base32::encode(self.0.expose_secret());
        let checksum = checksum_chars(account_id, self.0.expose_secret());

        let mut parts: Vec<String> = Vec::with_capacity(2 + 6 + 1);
        parts.push(VERSION_TAG.to_string());
        parts.push(account);
        parts.push(base32::group(&secret, SECRET_GROUP));
        parts.push(checksum);
        parts.join("-")
    }

    /// Parse an Emergency-Kit string back into the embedded account id
    /// and the Secret Key, verifying the checksum first.
    ///
    /// Case-insensitive; dashes and spaces are ignored; the grouping is
    /// purely cosmetic (parsing is length-based, not dash-based).
    ///
    /// # Errors
    /// Returns [`Error::InvalidSecretKey`] on a wrong version tag, wrong
    /// length, undecodable body, or a failed checksum.
    pub fn parse(input: &str) -> Result<([u8; ACCOUNT_ID_BYTES], Self), Error> {
        let compact: String = input
            .chars()
            .filter(|c| !matches!(c, '-' | ' '))
            .flat_map(char::to_uppercase)
            .collect();

        let expected_len = VERSION_TAG.len() + ACCOUNT_CHARS + SECRET_CHARS + CHECKSUM_CHARS;
        if compact.len() != expected_len {
            return Err(Error::InvalidSecretKey);
        }
        let (tag, rest) = compact.split_at(VERSION_TAG.len());
        if tag != VERSION_TAG {
            return Err(Error::InvalidSecretKey);
        }
        let (account_str, rest) = rest.split_at(ACCOUNT_CHARS);
        let (secret_str, checksum_str) = rest.split_at(SECRET_CHARS);

        let account_bytes =
            base32::decode(account_str, ACCOUNT_ID_BYTES).map_err(|_| Error::InvalidSecretKey)?;
        let secret_bytes =
            base32::decode(secret_str, SECRET_KEY_BYTES).map_err(|_| Error::InvalidSecretKey)?;

        let account_arr: [u8; ACCOUNT_ID_BYTES] = account_bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::InvalidSecretKey)?;
        let secret_arr: [u8; SECRET_KEY_BYTES] = secret_bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::InvalidSecretKey)?;

        let expected_checksum = checksum_chars(&account_arr, &secret_arr);
        if checksum_str != expected_checksum {
            return Err(Error::InvalidSecretKey);
        }

        Ok((account_arr, Self::from_bytes(secret_arr)))
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey<REDACTED>")
    }
}

/// Compute the transcription checksum group: a truncated SHA-256 over
/// the version tag, account id, and secret, encoded as Crockford Base32
/// and clipped to [`CHECKSUM_CHARS`] characters.
fn checksum_chars(account_id: &[u8; ACCOUNT_ID_BYTES], secret: &[u8; SECRET_KEY_BYTES]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(VERSION_TAG.as_bytes());
    hasher.update(account_id);
    hasher.update(secret);
    let digest = hasher.finalize();
    // 3 digest bytes → 5 Crockford chars; clip to the fixed group width.
    let mut chars = base32::encode(&digest[..3]);
    chars.truncate(CHECKSUM_CHARS);
    chars
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{ACCOUNT_ID_BYTES, CHECKSUM_CHARS, SecretKey, VERSION_TAG};

    fn sample_account() -> [u8; ACCOUNT_ID_BYTES] {
        [0x11; ACCOUNT_ID_BYTES]
    }

    #[test]
    fn generate_is_nonzero_and_random() {
        let a = SecretKey::generate().expect("gen");
        let b = SecretKey::generate().expect("gen");
        assert_ne!(a.expose_secret(), &[0_u8; 16]);
        assert_ne!(a.expose_secret(), b.expose_secret());
    }

    #[test]
    fn debug_is_redacted() {
        let sk = SecretKey::from_bytes([0xAB; 16]);
        let s = format!("{sk:?}");
        assert!(s.contains("REDACTED"));
        assert!(!s.contains("ab"));
        assert!(!s.contains("AB"));
    }

    #[test]
    fn emergency_kit_roundtrips() {
        let account = sample_account();
        let sk = SecretKey::from_bytes([0x42; 16]);
        let kit = sk.to_emergency_kit(&account);
        assert!(kit.starts_with(VERSION_TAG));

        let (parsed_account, parsed) = SecretKey::parse(&kit).expect("parse");
        assert_eq!(parsed_account, account);
        assert_eq!(parsed.expose_secret(), sk.expose_secret());
    }

    #[test]
    fn parse_is_case_and_dash_insensitive() {
        let account = sample_account();
        let sk = SecretKey::from_bytes([0x7E; 16]);
        let kit = sk.to_emergency_kit(&account);

        let messy = kit.to_lowercase().replace('-', "  ");
        let (parsed_account, parsed) = SecretKey::parse(&messy).expect("parse");
        assert_eq!(parsed_account, account);
        assert_eq!(parsed.expose_secret(), sk.expose_secret());
    }

    #[test]
    fn parse_rejects_bad_checksum() {
        let account = sample_account();
        let sk = SecretKey::from_bytes([0x01; 16]);
        let mut kit = sk.to_emergency_kit(&account);
        // Flip the last checksum character to a different valid symbol.
        let last = kit.pop().expect("nonempty");
        let replacement = if last == '0' { '1' } else { '0' };
        kit.push(replacement);
        assert!(matches!(
            SecretKey::parse(&kit),
            Err(crate::Error::InvalidSecretKey)
        ));
    }

    #[test]
    fn parse_rejects_wrong_version_tag() {
        let account = sample_account();
        let sk = SecretKey::from_bytes([0x01; 16]);
        let kit = sk.to_emergency_kit(&account);
        let tampered = format!("B4{}", &kit[VERSION_TAG.len()..]);
        assert!(matches!(
            SecretKey::parse(&tampered),
            Err(crate::Error::InvalidSecretKey)
        ));
    }

    #[test]
    fn parse_rejects_wrong_length() {
        assert!(matches!(
            SecretKey::parse("A4-0000"),
            Err(crate::Error::InvalidSecretKey)
        ));
    }

    #[test]
    fn checksum_group_has_fixed_width() {
        let kit = SecretKey::from_bytes([0xFF; 16]).to_emergency_kit(&sample_account());
        let last_group = kit.rsplit('-').next().expect("group");
        assert_eq!(last_group.len(), CHECKSUM_CHARS);
    }
}
