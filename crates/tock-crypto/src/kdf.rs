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

// ── Two-secret key derivation (2SKD) ─────────────────────────────────

/// HKDF `info` label for the Secret-Key stream in 2SKD (ADR-011).
const INFO_2SKD_SECRET_KEY: &[u8] = b"Tock/2skd/v1/secret-key";

/// HKDF `info` label for the SRP private-exponent input (ADR-011 §4).
const INFO_SRP_X: &[u8] = b"Tock/v1/srp-x";

/// XOR two 32-byte streams into a fresh [`SecretBytes`].
///
/// Used to combine the password and Secret-Key streams in 2SKD. XOR is
/// information-theoretically clean: neither input is recoverable from
/// the output without the other.
#[must_use]
fn xor_combine(a: &SecretBytes<32>, b: &SecretBytes<32>) -> SecretBytes<32> {
    let (a, b) = (a.expose_secret(), b.expose_secret());
    let mut out = [0_u8; 32];
    for (o, (x, y)) in out.iter_mut().zip(a.iter().zip(b.iter())) {
        *o = x ^ y;
    }
    SecretBytes::new(out)
}

/// Derive the **Unlock Root Key (URK)** from *both* account secrets
/// (ADR-011 §2):
///
/// ```text
/// K_pw = Argon2id(password, salt, params)                                  // something you know
/// K_sk = HKDF-SHA256(ikm = secret_key, salt = account_id ‖ kdf_version,
///                    info = "Tock/2skd/v1/secret-key")                     // something you have
/// URK  = K_pw XOR K_sk
/// ```
///
/// The `kdf_version` is folded into the Secret-Key HKDF salt so bumping
/// it changes the URK (forward-compatible re-wrap migrations). The URK
/// replaces the password-only Master Key at the root of the vault key
/// hierarchy and also seeds [`derive_srp_input`].
///
/// # Errors
/// Returns [`Error::InvalidArgon2Params`] / [`Error::Argon2`] if the
/// password hash fails, or [`Error::Hkdf`] if the Secret-Key expansion
/// fails (should not happen for a 32-byte output).
pub fn derive_unlock_root_key(
    password: &[u8],
    secret_key: &[u8],
    salt: &[u8; 16],
    account_id: &[u8; 16],
    kdf_version: u16,
    params: Argon2Params,
) -> Result<SecretBytes<32>, Error> {
    let k_pw = argon2id(password, salt, params)?;
    // Salt = account_id ‖ kdf_version (little-endian).
    let mut sk_salt = [0_u8; 18];
    sk_salt[..16].copy_from_slice(account_id);
    sk_salt[16..].copy_from_slice(&kdf_version.to_le_bytes());
    let k_sk = hkdf_sha256_32(secret_key, &sk_salt, INFO_2SKD_SECRET_KEY)?;
    Ok(xor_combine(&k_pw, &k_sk))
}

/// Derive the SRP private-exponent input from the URK (ADR-011 §4).
///
/// Returns the raw 32-byte HKDF output `HKDF-SHA256(ikm = URK,
/// salt = salt_srp, info = "Tock/v1/srp-x")`. Reduction `mod N`
/// (RFC 5054) is the responsibility of the SRP layer.
///
/// # Errors
/// Returns [`Error::Hkdf`] only if HKDF rejects the inputs.
pub fn derive_srp_input(urk: &SecretBytes<32>, salt_srp: &[u8]) -> Result<SecretBytes<32>, Error> {
    hkdf_sha256_32(urk.expose_secret(), salt_srp, INFO_SRP_X)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{Argon2Params, argon2id, hkdf_sha256, hkdf_sha256_32};
    use super::{derive_srp_input, derive_unlock_root_key};
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

    // ── 2SKD ─────────────────────────────────────────────────────────

    #[test]
    fn urk_is_deterministic() {
        let a = derive_unlock_root_key(
            b"pw",
            b"secret-key-bytes",
            &[7; 16],
            &[9; 16],
            1,
            TEST_PARAMS,
        )
        .expect("urk");
        let b = derive_unlock_root_key(
            b"pw",
            b"secret-key-bytes",
            &[7; 16],
            &[9; 16],
            1,
            TEST_PARAMS,
        )
        .expect("urk");
        assert_eq!(a, b);
    }

    #[test]
    fn urk_changes_when_password_changes() {
        let a =
            derive_unlock_root_key(b"pw1", b"sk", &[7; 16], &[9; 16], 1, TEST_PARAMS).expect("urk");
        let b =
            derive_unlock_root_key(b"pw2", b"sk", &[7; 16], &[9; 16], 1, TEST_PARAMS).expect("urk");
        assert_ne!(a, b);
    }

    #[test]
    fn urk_changes_when_secret_key_changes() {
        let a = derive_unlock_root_key(b"pw", b"sk-a", &[7; 16], &[9; 16], 1, TEST_PARAMS)
            .expect("urk");
        let b = derive_unlock_root_key(b"pw", b"sk-b", &[7; 16], &[9; 16], 1, TEST_PARAMS)
            .expect("urk");
        assert_ne!(a, b);
    }

    #[test]
    fn urk_changes_when_account_id_or_version_changes() {
        let base =
            derive_unlock_root_key(b"pw", b"sk", &[7; 16], &[9; 16], 1, TEST_PARAMS).expect("urk");
        let other_account =
            derive_unlock_root_key(b"pw", b"sk", &[7; 16], &[10; 16], 1, TEST_PARAMS).expect("urk");
        let other_version =
            derive_unlock_root_key(b"pw", b"sk", &[7; 16], &[9; 16], 2, TEST_PARAMS).expect("urk");
        assert_ne!(base, other_account);
        assert_ne!(base, other_version);
    }

    #[test]
    fn srp_input_tracks_both_secrets() {
        let salt_srp = b"srp-salt";
        let urk1 =
            derive_unlock_root_key(b"pw", b"sk", &[7; 16], &[9; 16], 1, TEST_PARAMS).expect("urk");
        let urk2 =
            derive_unlock_root_key(b"PW", b"sk", &[7; 16], &[9; 16], 1, TEST_PARAMS).expect("urk");
        let urk3 =
            derive_unlock_root_key(b"pw", b"SK", &[7; 16], &[9; 16], 1, TEST_PARAMS).expect("urk");

        let x1 = derive_srp_input(&urk1, salt_srp).expect("x");
        let x2 = derive_srp_input(&urk2, salt_srp).expect("x");
        let x3 = derive_srp_input(&urk3, salt_srp).expect("x");
        // Changing either secret changes the URK and therefore the SRP input.
        assert_ne!(x1, x2);
        assert_ne!(x1, x3);
        // Same inputs are deterministic.
        assert_eq!(x1, derive_srp_input(&urk1, salt_srp).expect("x"));
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 16, .. ProptestConfig::default() })]

        #[test]
        fn proptest_either_secret_change_flips_urk_and_srp(
            pw_a in proptest::collection::vec(any::<u8>(), 1..16),
            pw_b in proptest::collection::vec(any::<u8>(), 1..16),
            sk_a in proptest::collection::vec(any::<u8>(), 16..32),
            sk_b in proptest::collection::vec(any::<u8>(), 16..32),
            salt in any::<[u8; 16]>(),
            account in any::<[u8; 16]>(),
            salt_srp in proptest::collection::vec(any::<u8>(), 0..32),
        ) {
            prop_assume!(pw_a != pw_b);
            prop_assume!(sk_a != sk_b);

            let base = derive_unlock_root_key(&pw_a, &sk_a, &salt, &account, 1, TEST_PARAMS).expect("urk");
            let diff_pw = derive_unlock_root_key(&pw_b, &sk_a, &salt, &account, 1, TEST_PARAMS).expect("urk");
            let diff_sk = derive_unlock_root_key(&pw_a, &sk_b, &salt, &account, 1, TEST_PARAMS).expect("urk");

            prop_assert_ne!(&base, &diff_pw);
            prop_assert_ne!(&base, &diff_sk);

            let x_base = derive_srp_input(&base, &salt_srp).expect("x");
            prop_assert_ne!(&x_base, &derive_srp_input(&diff_pw, &salt_srp).expect("x"));
            prop_assert_ne!(&x_base, &derive_srp_input(&diff_sk, &salt_srp).expect("x"));
        }
    }
}
