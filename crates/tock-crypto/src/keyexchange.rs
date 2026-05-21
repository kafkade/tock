//! X25519 Diffie-Hellman key agreement.
//!
//! Two patterns are exposed:
//!
//! - [`StaticSecret`] for long-lived identity keys (e.g. a device's
//!   long-term agreement key).
//! - [`EphemeralSecret`] for per-session keys; the secret is consumed
//!   by [`EphemeralSecret::diffie_hellman`] so it cannot be re-used.
//!
//! All Diffie-Hellman operations [`reject`] the all-zero shared
//! secret. Although RFC 7748 says the small-order check is optional,
//! we always perform it because the workspace cannot afford to assume
//! callers will. See architecture §5.6 for how the shared secret is
//! used in the SRP-augmented channel binding.
//!
//! [`reject`]: Error::ContributorySharedSecret

use subtle::ConstantTimeEq;
use x25519_dalek::{EphemeralSecret as XEphemeral, PublicKey as XPublic, StaticSecret as XStatic};
use zeroize::Zeroize;

use crate::Error;
use crate::random::fill_random;
use crate::secret::SecretBytes;

/// X25519 32-byte public key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    /// Construct from raw bytes (no point validation — X25519 accepts
    /// any 32-byte input, with low-order points filtered at DH time).
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// View the raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Long-lived X25519 secret. Use for identity keys; for per-session
/// agreement prefer [`EphemeralSecret`].
pub struct StaticSecret(XStatic);

impl StaticSecret {
    /// Generate a fresh `StaticSecret` from the OS RNG.
    ///
    /// # Errors
    /// Returns [`Error::Rng`] if the OS RNG fails.
    pub fn try_generate() -> Result<Self, Error> {
        let mut seed = [0_u8; 32];
        fill_random(&mut seed)?;
        let inner = XStatic::from(seed);
        seed.zeroize();
        Ok(Self(inner))
    }

    /// Reconstruct a `StaticSecret` from its 32-byte canonical encoding.
    ///
    /// Note: X25519 clamps the input internally — there are no invalid
    /// 32-byte inputs.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(XStatic::from(bytes))
    }

    /// Derive the corresponding public key.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        let xp = XPublic::from(&self.0);
        PublicKey(xp.to_bytes())
    }

    /// Compute the X25519 shared secret with `peer`.
    ///
    /// # Errors
    /// Returns [`Error::ContributorySharedSecret`] if the result is
    /// all-zero, indicating a low-order or otherwise contributory peer
    /// public key.
    pub fn diffie_hellman(&self, peer: &PublicKey) -> Result<SharedSecret, Error> {
        let peer = XPublic::from(*peer.as_bytes());
        let shared = self.0.diffie_hellman(&peer);
        reject_if_zero(shared.to_bytes())
    }
}

impl core::fmt::Debug for StaticSecret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("StaticSecret<REDACTED>")
    }
}

/// Single-use X25519 secret. Consumed by [`EphemeralSecret::diffie_hellman`]
/// so the same secret cannot be used in two agreements.
pub struct EphemeralSecret(XEphemeral);

impl EphemeralSecret {
    /// Generate a fresh `EphemeralSecret` from the OS RNG.
    ///
    /// # Errors
    /// Returns [`Error::Rng`] if the OS RNG fails.
    pub fn try_generate() -> Result<Self, Error> {
        // x25519-dalek 2.x requires `RngCore + CryptoRng`. OsRng with
        // the `getrandom` feature satisfies both.
        Ok(Self(XEphemeral::random_from_rng(rand_core::OsRng)))
    }

    /// Derive the corresponding public key.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        let xp = XPublic::from(&self.0);
        PublicKey(xp.to_bytes())
    }

    /// Consume the secret and produce the shared secret with `peer`.
    ///
    /// # Errors
    /// Returns [`Error::ContributorySharedSecret`] if the result is
    /// all-zero.
    pub fn diffie_hellman(self, peer: &PublicKey) -> Result<SharedSecret, Error> {
        let peer = XPublic::from(*peer.as_bytes());
        let shared = self.0.diffie_hellman(&peer);
        reject_if_zero(shared.to_bytes())
    }
}

impl core::fmt::Debug for EphemeralSecret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("EphemeralSecret<REDACTED>")
    }
}

/// 32-byte X25519 shared secret. Use as IKM for [`crate::kdf::hkdf_sha256`].
pub struct SharedSecret(SecretBytes<32>);

impl SharedSecret {
    /// Borrow the inner secret bytes.
    #[must_use]
    pub const fn as_secret(&self) -> &SecretBytes<32> {
        &self.0
    }
}

impl core::fmt::Debug for SharedSecret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SharedSecret<REDACTED>")
    }
}

fn reject_if_zero(bytes: [u8; 32]) -> Result<SharedSecret, Error> {
    // Constant-time comparison against the all-zero buffer.
    if bool::from(bytes.ct_eq(&[0_u8; 32])) {
        // `bytes` is already on the stack; zeroize is a courtesy.
        let mut z = bytes;
        z.zeroize();
        return Err(Error::ContributorySharedSecret);
    }
    Ok(SharedSecret(SecretBytes::new(bytes)))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{EphemeralSecret, PublicKey, StaticSecret};
    use crate::Error;

    #[test]
    fn static_static_dh_is_symmetric() {
        let a = StaticSecret::try_generate().expect("rng");
        let b = StaticSecret::try_generate().expect("rng");
        let ab = a.diffie_hellman(&b.public_key()).expect("dh");
        let ba = b.diffie_hellman(&a.public_key()).expect("dh");
        assert_eq!(ab.as_secret(), ba.as_secret());
    }

    #[test]
    fn static_ephemeral_dh_is_symmetric() {
        let server = StaticSecret::try_generate().expect("rng");
        let client = EphemeralSecret::try_generate().expect("rng");
        let client_pub = client.public_key();
        let from_client = client.diffie_hellman(&server.public_key()).expect("dh");
        let from_server = server.diffie_hellman(&client_pub).expect("dh");
        assert_eq!(from_client.as_secret(), from_server.as_secret());
    }

    #[test]
    fn all_zero_peer_public_key_rejected() {
        let a = StaticSecret::try_generate().expect("rng");
        let zero = PublicKey::from_bytes([0; 32]);
        let result = a.diffie_hellman(&zero);
        assert!(matches!(result, Err(Error::ContributorySharedSecret)));
    }

    #[test]
    fn one_low_order_peer_public_key_rejected() {
        // X25519 small-order point: u=1, also produces all-zero shared.
        let a = StaticSecret::try_generate().expect("rng");
        let mut p = [0_u8; 32];
        p[0] = 1;
        let low = PublicKey::from_bytes(p);
        assert!(matches!(
            a.diffie_hellman(&low),
            Err(Error::ContributorySharedSecret)
        ));
    }

    #[test]
    fn from_bytes_roundtrips_public_key() {
        let s = StaticSecret::try_generate().expect("rng");
        let bytes = *s.public_key().as_bytes();
        let p = PublicKey::from_bytes(bytes);
        assert_eq!(p.as_bytes(), &bytes);
    }
}
