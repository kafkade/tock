//! Fixed-size secret byte container with constant-time equality and
//! automatic zeroization on drop.
//!
//! Use [`SecretBytes`] to hold any symmetric key, derived KDF output,
//! or other 32 (or N) byte secret. Raw access is gated behind
//! [`SecretBytes::expose_secret`] so it grep-greps clearly in code
//! review.

use core::fmt;

use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::Error;
use crate::random::fill_random;

/// `N` bytes of secret material. Zeroized on drop. Equality is
/// constant-time.
///
/// `SecretBytes` deliberately does **not** implement `Clone`, `Copy`,
/// `Debug`-with-bytes, `Serialize`, or `Display`. The redacted
/// [`fmt::Debug`] impl prints only the length.
///
/// To obtain a copy, call [`SecretBytes::clone_secret`] explicitly —
/// this name is greppable.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecretBytes<const N: usize>([u8; N]);

impl<const N: usize> SecretBytes<N> {
    /// Wrap pre-existing bytes as a secret.
    ///
    /// Note: the caller's `bytes` array is moved into the secret, but
    /// any prior copies in caller-controlled memory are not the
    /// responsibility of this type and must be zeroized separately.
    #[must_use]
    pub const fn new(bytes: [u8; N]) -> Self {
        Self(bytes)
    }

    /// Fill a fresh `SecretBytes<N>` from the operating system RNG.
    ///
    /// # Errors
    /// Returns [`Error::Rng`] if the OS RNG fails.
    pub fn try_random() -> Result<Self, Error> {
        let mut bytes = [0_u8; N];
        fill_random(&mut bytes)?;
        Ok(Self(bytes))
    }

    /// Borrow the underlying secret bytes.
    ///
    /// **Hazmat.** Prefer passing the [`SecretBytes`] handle itself
    /// around; only call `expose_secret` at the boundary where the raw
    /// bytes must be handed to a primitive (e.g. an AEAD key).
    #[must_use]
    pub const fn expose_secret(&self) -> &[u8; N] {
        &self.0
    }

    /// Deliberate, greppable copy. Use sparingly.
    #[must_use]
    pub const fn clone_secret(&self) -> Self {
        Self(self.0)
    }

    /// Length in bytes (always `N`).
    #[must_use]
    pub const fn len(&self) -> usize {
        N
    }

    /// Always false for `N > 0`. Provided so `clippy::len_without_is_empty` is satisfied.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        N == 0
    }
}

impl<const N: usize> ConstantTimeEq for SecretBytes<N> {
    fn ct_eq(&self, other: &Self) -> subtle::Choice {
        self.0.ct_eq(&other.0)
    }
}

impl<const N: usize> PartialEq for SecretBytes<N> {
    fn eq(&self, other: &Self) -> bool {
        self.ct_eq(other).into()
    }
}

impl<const N: usize> Eq for SecretBytes<N> {}

impl<const N: usize> fmt::Debug for SecretBytes<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretBytes<REDACTED; {N} bytes>")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::missing_const_for_fn)]

    use super::SecretBytes;

    #[test]
    fn debug_is_redacted() {
        let s = SecretBytes::<32>::new([0xAA; 32]);
        let formatted = format!("{s:?}");
        assert!(formatted.contains("REDACTED"));
        assert!(!formatted.contains("aa"));
        assert!(!formatted.contains("AA"));
    }

    #[test]
    fn equal_secrets_compare_equal() {
        let a = SecretBytes::<32>::new([1; 32]);
        let b = SecretBytes::<32>::new([1; 32]);
        assert_eq!(a, b);
    }

    #[test]
    fn differing_secrets_compare_not_equal() {
        let a = SecretBytes::<32>::new([1; 32]);
        let mut other = [1_u8; 32];
        other[17] = 2;
        let b = SecretBytes::<32>::new(other);
        assert_ne!(a, b);
    }

    #[test]
    fn clone_secret_produces_equal_handle() {
        let a = SecretBytes::<16>::new([7; 16]);
        let b = a.clone_secret();
        assert_eq!(a, b);
    }

    #[test]
    fn try_random_produces_nonconstant_bytes() {
        let a = SecretBytes::<32>::try_random().expect("rng");
        let b = SecretBytes::<32>::try_random().expect("rng");
        assert_ne!(a, b);
        assert_ne!(a.expose_secret(), &[0_u8; 32]);
    }
}
