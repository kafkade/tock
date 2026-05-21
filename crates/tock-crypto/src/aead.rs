//! AES-256-GCM authenticated encryption with associated data.
//!
//! All AEAD operations in tock use AES-256-GCM with 96-bit nonces and
//! 128-bit tags (the [`aes_gcm`] defaults).
//!
//! ## Nonce discipline
//!
//! **Re-using the same `(Key, Nonce)` pair across two distinct
//! plaintexts is catastrophic** — the keystream XOR leaks both
//! plaintexts and authentication is silently broken. Always derive
//! nonces from [`Nonce::try_random`] (96-bit random nonces are
//! birthday-safe for `≲ 2³² messages per key`) or from a
//! deterministic, domain-separated derivation rooted in the event
//! identifier.
//!
//! ## AAD
//!
//! AAD is opaque to this layer: callers are responsible for
//! domain-separated, canonical serialization of the context tuple
//! (`entity_kind`, `entity_id`, `op`, `lamport`, `device_id`, …) per
//! architecture §5.3. This crate refuses to bake that format in
//! because it would couple cryptography to domain types.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key as AesKey, Nonce as AesNonce};
use zeroize::Zeroizing;

use crate::Error;
use crate::secret::SecretBytes;

/// 256-bit symmetric AEAD key.
pub struct Key(SecretBytes<32>);

impl Key {
    /// Wrap pre-existing 32 random bytes as a key.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(SecretBytes::new(bytes))
    }

    /// Wrap an already-zeroizing secret as a key.
    #[must_use]
    pub const fn from_secret(secret: SecretBytes<32>) -> Self {
        Self(secret)
    }

    /// Generate a fresh random key from the OS RNG.
    ///
    /// # Errors
    /// Returns [`Error::Rng`] if the OS RNG fails.
    pub fn try_generate() -> Result<Self, Error> {
        Ok(Self(SecretBytes::try_random()?))
    }

    /// Borrow the inner [`SecretBytes`].
    #[must_use]
    pub const fn as_secret(&self) -> &SecretBytes<32> {
        &self.0
    }
}

impl core::fmt::Debug for Key {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("aead::Key<REDACTED>")
    }
}

/// 96-bit AES-GCM nonce.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Nonce([u8; 12]);

impl Nonce {
    /// Construct a nonce from raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 12]) -> Self {
        Self(bytes)
    }

    /// Generate a fresh random nonce from the OS RNG.
    ///
    /// # Errors
    /// Returns [`Error::Rng`] if the OS RNG fails.
    pub fn try_random() -> Result<Self, Error> {
        let mut bytes = [0_u8; 12];
        crate::random::fill_random(&mut bytes)?;
        Ok(Self(bytes))
    }

    /// View the underlying bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 12] {
        &self.0
    }
}

fn cipher(key: &Key) -> Aes256Gcm {
    let raw: &[u8; 32] = key.0.expose_secret();
    Aes256Gcm::new(AesKey::<Aes256Gcm>::from_slice(raw))
}

/// Encrypt `plaintext` under `(key, nonce, aad)`, returning
/// `ciphertext || tag` (the 16-byte authentication tag appended).
///
/// # Errors
/// Returns [`Error::AeadEncrypt`] if the plaintext is larger than the
/// AES-GCM maximum message size (`~64 GiB`).
pub fn seal(key: &Key, nonce: &Nonce, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Error> {
    let nonce = AesNonce::from_slice(nonce.as_bytes());
    cipher(key)
        .encrypt(
            nonce,
            Payload {
                aad,
                msg: plaintext,
            },
        )
        .map_err(|_| Error::AeadEncrypt)
}

/// Authenticate and decrypt `ciphertext` (expected to be `ct || tag`),
/// returning the plaintext wrapped in a [`Zeroizing`] container so it
/// is wiped when dropped.
///
/// # Errors
/// Returns [`Error::Aead`] on any authentication failure — wrong key,
/// wrong nonce, wrong AAD, truncated ciphertext, or bit-flipped tag.
/// The failure cause is deliberately not distinguished.
pub fn open(
    key: &Key,
    nonce: &Nonce,
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, Error> {
    let nonce = AesNonce::from_slice(nonce.as_bytes());
    let pt = cipher(key)
        .decrypt(
            nonce,
            Payload {
                aad,
                msg: ciphertext,
            },
        )
        .map_err(|_| Error::Aead)?;
    Ok(Zeroizing::new(pt))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::missing_const_for_fn)]

    use super::{Key, Nonce, open, seal};
    use proptest::prelude::*;

    fn fixed_key() -> Key {
        Key::from_bytes([0x42; 32])
    }
    fn fixed_nonce() -> Nonce {
        Nonce::from_bytes([0x07; 12])
    }

    #[test]
    fn roundtrip_simple() {
        let key = fixed_key();
        let nonce = fixed_nonce();
        let aad = b"tock|v1|test";
        let pt = b"the quick brown fox".to_vec();
        let ct = seal(&key, &nonce, aad, &pt).expect("encrypt");
        assert_eq!(ct.len(), pt.len() + 16, "ct should be pt + tag");
        let recovered = open(&key, &nonce, aad, &ct).expect("decrypt");
        assert_eq!(recovered.as_slice(), pt.as_slice());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = fixed_key();
        let nonce = fixed_nonce();
        let aad = b"";
        let mut ct = seal(&key, &nonce, aad, b"hello").expect("encrypt");
        ct[0] ^= 0x01;
        assert!(open(&key, &nonce, aad, &ct).is_err());
    }

    #[test]
    fn tampered_tag_fails() {
        let key = fixed_key();
        let nonce = fixed_nonce();
        let aad = b"";
        let mut ct = seal(&key, &nonce, aad, b"hello").expect("encrypt");
        let last = ct.len() - 1;
        ct[last] ^= 0x01;
        assert!(open(&key, &nonce, aad, &ct).is_err());
    }

    #[test]
    fn wrong_aad_fails() {
        let key = fixed_key();
        let nonce = fixed_nonce();
        let ct = seal(&key, &nonce, b"aad-a", b"hello").expect("encrypt");
        assert!(open(&key, &nonce, b"aad-b", &ct).is_err());
    }

    #[test]
    fn wrong_key_fails() {
        let nonce = fixed_nonce();
        let ct = seal(&fixed_key(), &nonce, b"", b"hello").expect("encrypt");
        let other = Key::from_bytes([0x01; 32]);
        assert!(open(&other, &nonce, b"", &ct).is_err());
    }

    #[test]
    fn wrong_nonce_fails() {
        let key = fixed_key();
        let ct = seal(&key, &fixed_nonce(), b"", b"hello").expect("encrypt");
        let other_nonce = Nonce::from_bytes([0x08; 12]);
        assert!(open(&key, &other_nonce, b"", &ct).is_err());
    }

    #[test]
    fn empty_plaintext_roundtrips() {
        let key = fixed_key();
        let nonce = fixed_nonce();
        let ct = seal(&key, &nonce, b"aad", b"").expect("encrypt");
        // 0 plaintext + 16 tag = 16 bytes.
        assert_eq!(ct.len(), 16);
        let pt = open(&key, &nonce, b"aad", &ct).expect("decrypt");
        assert!(pt.is_empty());
    }

    #[test]
    fn try_generate_keys_are_random() {
        let a = Key::try_generate().expect("rng");
        let b = Key::try_generate().expect("rng");
        assert_ne!(a.as_secret(), b.as_secret());
    }

    #[test]
    fn try_random_nonces_are_random() {
        let a = Nonce::try_random().expect("rng");
        let b = Nonce::try_random().expect("rng");
        assert_ne!(a, b);
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, .. ProptestConfig::default() })]

        #[test]
        fn proptest_roundtrip(
            key_bytes in any::<[u8; 32]>(),
            nonce_bytes in any::<[u8; 12]>(),
            aad in proptest::collection::vec(any::<u8>(), 0..64),
            pt in proptest::collection::vec(any::<u8>(), 0..256),
        ) {
            let key = Key::from_bytes(key_bytes);
            let nonce = Nonce::from_bytes(nonce_bytes);
            let ct = seal(&key, &nonce, &aad, &pt).expect("encrypt");
            let recovered = open(&key, &nonce, &aad, &ct).expect("decrypt");
            prop_assert_eq!(recovered.as_slice(), pt.as_slice());
        }

        #[test]
        fn proptest_bit_flip_anywhere_fails(
            key_bytes in any::<[u8; 32]>(),
            nonce_bytes in any::<[u8; 12]>(),
            pt in proptest::collection::vec(any::<u8>(), 1..128),
            flip_index in any::<usize>(),
        ) {
            let key = Key::from_bytes(key_bytes);
            let nonce = Nonce::from_bytes(nonce_bytes);
            let mut ct = seal(&key, &nonce, b"", &pt).expect("encrypt");
            let i = flip_index % ct.len();
            ct[i] ^= 0x01;
            prop_assert!(open(&key, &nonce, b"", &ct).is_err());
        }
    }
}
