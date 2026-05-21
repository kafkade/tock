//! OS RNG helper.
//!
//! All randomness in `tock-crypto` flows through [`fill_random`]. The
//! function is fallible because the workspace forbids panics; the
//! underlying `RngCore::fill_bytes` aborts on OS RNG failure, which we
//! avoid by calling `try_fill_bytes` instead.

use rand_core::{OsRng, RngCore};

use crate::Error;

/// Fill `dest` with cryptographically-secure random bytes from the
/// operating system RNG.
///
/// # Errors
/// Returns [`Error::Rng`] if the OS RNG fails (e.g. blocked entropy
/// pool on early-boot Linux, sandbox restrictions).
pub fn fill_random(dest: &mut [u8]) -> Result<(), Error> {
    OsRng.try_fill_bytes(dest).map_err(|_| Error::Rng)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::fill_random;

    #[test]
    fn fills_buffer_with_nonzero_entropy() {
        let mut a = [0_u8; 64];
        let mut b = [0_u8; 64];
        fill_random(&mut a).expect("rng");
        fill_random(&mut b).expect("rng");
        assert_ne!(a, b);
        assert!(a.iter().any(|&x| x != 0));
    }

    #[test]
    fn empty_buffer_succeeds() {
        let mut empty: [u8; 0] = [];
        fill_random(&mut empty).expect("rng");
    }
}
