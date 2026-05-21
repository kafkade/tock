//! Ed25519 signatures.
//!
//! Only the non-prehashed `sign(&[u8])` / `verify(&[u8], &Signature)`
//! interface is exposed. Prehashed signatures (Ed25519ph) are not
//! enabled for this crate to keep the misuse surface small.
//!
//! Verifying-key bytes are validated on construction
//! ([`VerifyingKey::from_bytes`]).

use ed25519_dalek::{
    Signature as EdSignature, Signer, SigningKey as EdSigningKey, VerifyingKey as EdVerifyingKey,
};
use zeroize::Zeroize;

use crate::Error;
use crate::random::fill_random;

/// 32-byte Ed25519 signing key (the secret half of an Ed25519 keypair).
pub struct SigningKey(EdSigningKey);

impl SigningKey {
    /// Generate a fresh signing key from the OS RNG.
    ///
    /// # Errors
    /// Returns [`Error::Rng`] if the OS RNG fails.
    pub fn try_generate() -> Result<Self, Error> {
        let mut seed = [0_u8; 32];
        fill_random(&mut seed)?;
        let inner = EdSigningKey::from_bytes(&seed);
        seed.zeroize();
        Ok(Self(inner))
    }

    /// Reconstruct a `SigningKey` from its 32-byte canonical encoding.
    ///
    /// Note: any 32-byte input is a valid Ed25519 seed; there is no
    /// validation step.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(EdSigningKey::from_bytes(bytes))
    }

    /// Derive the corresponding verifying (public) key.
    #[must_use]
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey(self.0.verifying_key())
    }

    /// Sign `message`.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Signature {
        Signature(self.0.sign(message))
    }
}

impl core::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ed25519::SigningKey<REDACTED>")
    }
}

/// 32-byte Ed25519 verifying (public) key. Construction validates that
/// the bytes decode to a valid point.
#[derive(Clone, Copy, Debug)]
pub struct VerifyingKey(EdVerifyingKey);

impl VerifyingKey {
    /// Decode and validate a verifying key.
    ///
    /// # Errors
    /// Returns [`Error::InvalidEncoding`] if `bytes` does not decode to
    /// a valid Ed25519 point.
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self, Error> {
        EdVerifyingKey::from_bytes(bytes)
            .map(Self)
            .map_err(|_| Error::InvalidEncoding)
    }

    /// View the canonical 32-byte encoding.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// Verify `signature` over `message`.
    ///
    /// # Errors
    /// Returns [`Error::SignatureVerification`] if verification fails.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), Error> {
        self.0
            .verify_strict(message, &signature.0)
            .map_err(|_| Error::SignatureVerification)
    }
}

impl PartialEq for VerifyingKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bytes() == other.0.to_bytes()
    }
}

impl Eq for VerifyingKey {}

/// 64-byte Ed25519 signature.
#[derive(Clone, Copy, Debug)]
pub struct Signature(EdSignature);

impl Signature {
    /// Reconstruct a signature from its 64-byte canonical encoding.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; 64]) -> Self {
        Self(EdSignature::from_bytes(bytes))
    }

    /// The 64-byte canonical encoding.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 64] {
        self.0.to_bytes()
    }
}

impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bytes() == other.0.to_bytes()
    }
}

impl Eq for Signature {}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{Signature, SigningKey, VerifyingKey};
    use proptest::prelude::*;

    #[test]
    fn sign_and_verify_roundtrip() {
        let sk = SigningKey::try_generate().expect("rng");
        let vk = sk.verifying_key();
        let msg = b"audit me";
        let sig = sk.sign(msg);
        vk.verify(msg, &sig).expect("verify");
    }

    #[test]
    fn tampered_message_fails() {
        let sk = SigningKey::try_generate().expect("rng");
        let vk = sk.verifying_key();
        let sig = sk.sign(b"original");
        assert!(vk.verify(b"tampered", &sig).is_err());
    }

    #[test]
    fn tampered_signature_fails() {
        let sk = SigningKey::try_generate().expect("rng");
        let vk = sk.verifying_key();
        let mut bytes = sk.sign(b"msg").to_bytes();
        bytes[0] ^= 0x01;
        let sig = Signature::from_bytes(&bytes);
        assert!(vk.verify(b"msg", &sig).is_err());
    }

    #[test]
    fn wrong_verifying_key_fails() {
        let sk = SigningKey::try_generate().expect("rng");
        let other = SigningKey::try_generate().expect("rng");
        let sig = sk.sign(b"msg");
        assert!(other.verifying_key().verify(b"msg", &sig).is_err());
    }

    #[test]
    fn verifying_key_invalid_encoding_rejected() {
        // y = 2 with sign-bit 0 has no valid x (not a square mod p), so
        // Edwards point decompression fails. Verified empirically.
        let mut invalid = [0_u8; 32];
        invalid[0] = 0x02;
        assert!(VerifyingKey::from_bytes(&invalid).is_err());
    }

    #[test]
    fn signature_roundtrip_encoding() {
        let sk = SigningKey::try_generate().expect("rng");
        let sig = sk.sign(b"msg");
        let bytes = sig.to_bytes();
        let parsed = Signature::from_bytes(&bytes);
        assert_eq!(sig, parsed);
    }

    #[test]
    fn verifying_key_roundtrip_encoding() {
        let sk = SigningKey::try_generate().expect("rng");
        let vk = sk.verifying_key();
        let bytes = vk.to_bytes();
        let parsed = VerifyingKey::from_bytes(&bytes).expect("valid");
        assert_eq!(vk, parsed);
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 32, .. ProptestConfig::default() })]

        #[test]
        fn proptest_roundtrip(
            seed in any::<[u8; 32]>(),
            msg in proptest::collection::vec(any::<u8>(), 0..256),
        ) {
            let sk = SigningKey::from_bytes(&seed);
            let vk = sk.verifying_key();
            let sig = sk.sign(&msg);
            prop_assert!(vk.verify(&msg, &sig).is_ok());
        }

        #[test]
        fn proptest_message_bit_flip_fails(
            seed in any::<[u8; 32]>(),
            msg in proptest::collection::vec(any::<u8>(), 1..128),
            flip in any::<usize>(),
        ) {
            let sk = SigningKey::from_bytes(&seed);
            let vk = sk.verifying_key();
            let sig = sk.sign(&msg);
            let mut tampered = msg;
            let i = flip % tampered.len();
            tampered[i] ^= 0x01;
            prop_assert!(vk.verify(&tampered, &sig).is_err());
        }
    }
}
