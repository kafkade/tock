//! Key derivation: Argon2id password hashing and HKDF-SHA256.
//!
//! - [`argon2id`] is the password-hardening step at the top of the key
//!   hierarchy (architecture §5.1): `password → MK`.
//! - [`hkdf_sha256`] (and its 32-byte convenience [`hkdf_sha256_32`])
//!   derives every downstream key in the hierarchy (`MK → MEK`, `VK →
//!   DK_kind`, `DK_kind + entity_id → IK`) with domain-separated `info`.
//!
//! ## Parameters
//!
//! The vault-format default lives in [`Argon2Params::TOCK_V1`]:
//! `t=3, m=64 MiB, p=1`, matching the binary header in architecture §5.2.
//!
//! Construct other parameters with [`Argon2Params::new`], which
//! validates bounds and rejects pathological values that could enable
//! a denial-of-service via a malicious vault header.

use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::Error;
use crate::secret::SecretBytes;

/// Validated Argon2id parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Argon2Params {
    /// Iterations (`t`). Must be `>= 1`.
    pub t: u32,
    /// Memory cost in KiB (`m`). Bounded `[8, 4_194_304]` (8 KiB to 4 GiB).
    pub m_kib: u32,
    /// Parallelism (`p`). Bounded `[1, 64]`.
    pub p: u32,
}

impl Argon2Params {
    /// Canonical vault parameters — `t=3, m=64 MiB, p=1` per architecture §5.2.
    pub const TOCK_V1: Self = Self {
        t: 3,
        m_kib: 65_536,
        p: 1,
    };

    /// Build and validate parameters.
    ///
    /// # Errors
    /// Returns [`Error::InvalidArgon2Params`] if any value is out of
    /// bounds:
    /// - `t >= 1`
    /// - `m_kib` in `[8, 4_194_304]`
    /// - `p` in `[1, 64]`
    pub fn new(t: u32, m_kib: u32, p: u32) -> Result<Self, Error> {
        if t == 0 || !(8..=4_194_304).contains(&m_kib) || !(1..=64).contains(&p) {
            return Err(Error::InvalidArgon2Params);
        }
        Ok(Self { t, m_kib, p })
    }
}

/// Argon2id password hash. Produces a 32-byte secret rooted at the
/// user password, the per-vault 16-byte salt, and the chosen parameters.
///
/// # Errors
/// Returns [`Error::InvalidArgon2Params`] if `params` can't be
/// instantiated by the underlying library; [`Error::Argon2`] if
/// hashing itself fails (e.g. memory allocation failure for very
/// large `m_kib`).
pub fn argon2id(
    password: &[u8],
    salt: &[u8; 16],
    params: Argon2Params,
) -> Result<SecretBytes<32>, Error> {
    let cfg = Params::new(params.m_kib, params.t, params.p, Some(32))
        .map_err(|_| Error::InvalidArgon2Params)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, cfg);
    let mut out = [0_u8; 32];
    argon
        .hash_password_into(password, salt, &mut out)
        .map_err(|_| Error::Argon2)?;
    Ok(SecretBytes::new(out))
}

/// HKDF-SHA256 extract-and-expand into the caller-provided `output` buffer.
///
/// The caller controls allocation and zeroization. For the common
/// 32-byte case, use [`hkdf_sha256_32`].
///
/// # Errors
/// Returns [`Error::Hkdf`] if `output.len()` exceeds the HKDF-SHA256
/// maximum output length (255 × 32 = 8160 bytes).
pub fn hkdf_sha256(ikm: &[u8], salt: &[u8], info: &[u8], output: &mut [u8]) -> Result<(), Error> {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    hk.expand(info, output).map_err(|_| Error::Hkdf)
}

/// Convenience: HKDF-SHA256 producing a 32-byte [`SecretBytes`].
///
/// # Errors
/// Returns [`Error::Hkdf`] only if the underlying HKDF implementation
/// rejects the inputs (should not happen for a 32-byte output, since
/// `32 <= 8160`).
pub fn hkdf_sha256_32(ikm: &[u8], salt: &[u8], info: &[u8]) -> Result<SecretBytes<32>, Error> {
    let mut out = [0_u8; 32];
    hkdf_sha256(ikm, salt, info, &mut out)?;
    Ok(SecretBytes::new(out))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{Argon2Params, argon2id, hkdf_sha256, hkdf_sha256_32};
    use proptest::prelude::*;

    // Use tiny params for property testing; one focused test covers
    // TOCK_V1 below.
    const TEST_PARAMS: Argon2Params = Argon2Params {
        t: 1,
        m_kib: 8,
        p: 1,
    };

    #[test]
    fn argon2_deterministic_for_same_inputs() {
        let a = argon2id(b"hunter2", &[1; 16], TEST_PARAMS).expect("hash");
        let b = argon2id(b"hunter2", &[1; 16], TEST_PARAMS).expect("hash");
        assert_eq!(a, b);
    }

    #[test]
    fn argon2_different_password_different_output() {
        let a = argon2id(b"hunter2", &[1; 16], TEST_PARAMS).expect("hash");
        let b = argon2id(b"hunter3", &[1; 16], TEST_PARAMS).expect("hash");
        assert_ne!(a, b);
    }

    #[test]
    fn argon2_different_salt_different_output() {
        let a = argon2id(b"hunter2", &[1; 16], TEST_PARAMS).expect("hash");
        let b = argon2id(b"hunter2", &[2; 16], TEST_PARAMS).expect("hash");
        assert_ne!(a, b);
    }

    #[test]
    fn argon2_tock_v1_runs_and_is_32_bytes() {
        // Slower test; runs once.
        let h = argon2id(
            b"correct horse battery staple",
            &[9; 16],
            Argon2Params::TOCK_V1,
        )
        .expect("hash");
        assert_eq!(h.len(), 32);
    }

    #[test]
    fn argon2_params_rejects_bad_inputs() {
        assert!(Argon2Params::new(0, 64, 1).is_err());
        assert!(Argon2Params::new(1, 7, 1).is_err());
        assert!(Argon2Params::new(1, 4_194_305, 1).is_err());
        assert!(Argon2Params::new(1, 64, 0).is_err());
        assert!(Argon2Params::new(1, 64, 65).is_err());
        assert!(Argon2Params::new(1, 64, 1).is_ok());
        assert_eq!(
            Argon2Params::TOCK_V1,
            Argon2Params::new(3, 65_536, 1).expect("valid")
        );
    }

    #[test]
    fn hkdf_is_deterministic() {
        let a = hkdf_sha256_32(b"ikm", b"salt", b"info").expect("hkdf");
        let b = hkdf_sha256_32(b"ikm", b"salt", b"info").expect("hkdf");
        assert_eq!(a, b);
    }

    #[test]
    fn hkdf_different_info_different_output() {
        let a = hkdf_sha256_32(b"ikm", b"salt", b"info-a").expect("hkdf");
        let b = hkdf_sha256_32(b"ikm", b"salt", b"info-b").expect("hkdf");
        assert_ne!(a, b);
    }

    #[test]
    fn hkdf_different_lengths_first_bytes_match() {
        // HKDF-Expand is a prefix: the first 32 bytes of a 64-byte output
        // equal the 32-byte output.
        let small = hkdf_sha256_32(b"ikm", b"salt", b"info").expect("hkdf");
        let mut large = [0_u8; 64];
        hkdf_sha256(b"ikm", b"salt", b"info", &mut large).expect("hkdf");
        assert_eq!(small.expose_secret(), &large[..32]);
    }

    #[test]
    fn hkdf_oversized_output_rejected() {
        let mut huge = vec![0_u8; 8161]; // 255*32 = 8160 is the max
        assert!(hkdf_sha256(b"ikm", b"salt", b"info", &mut huge).is_err());
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 32, .. ProptestConfig::default() })]

        #[test]
        fn proptest_argon2_deterministic(
            password in proptest::collection::vec(any::<u8>(), 0..32),
            salt in any::<[u8; 16]>(),
        ) {
            let a = argon2id(&password, &salt, TEST_PARAMS).expect("hash");
            let b = argon2id(&password, &salt, TEST_PARAMS).expect("hash");
            prop_assert_eq!(a, b);
        }

        #[test]
        fn proptest_hkdf_deterministic(
            ikm in proptest::collection::vec(any::<u8>(), 1..64),
            salt in proptest::collection::vec(any::<u8>(), 0..64),
            info in proptest::collection::vec(any::<u8>(), 0..64),
        ) {
            let a = hkdf_sha256_32(&ikm, &salt, &info).expect("hkdf");
            let b = hkdf_sha256_32(&ikm, &salt, &info).expect("hkdf");
            prop_assert_eq!(a, b);
        }
    }
}
