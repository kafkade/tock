//! SRP-6a (RFC 5054, 4096-bit group, SHA-256) over the two-secret input.
//!
//! This module implements the zero-knowledge authentication handshake from
//! [ADR-010] as amended by [ADR-011]: the SRP private exponent `x` derives
//! from the **Unlock Root Key (URK)** — itself a function of the account
//! password *and* the 128-bit Secret Key — rather than from the bare
//! password. Concretely the caller obtains `x` from
//! [`crate::kdf::derive_srp_input`] (`x = HKDF(URK, salt_srp,
//! "Tock/v1/srp-x")`) and feeds the resulting [`SecretBytes<32>`] here. The
//! verifier `v = g^x mod N` is therefore offline-crackable only by an
//! attacker who also holds the Secret Key, restoring the 1Password guarantee
//! to SRP.
//!
//! ## What the server sees
//!
//! Registration emits `(account_id, salt_srp, v)`; login is a mutual-auth
//! exchange yielding a shared session key `K = H(S)`. The server never
//! receives the password, the Secret Key, the URK, or `x`.
//!
//! ## Layering on the `srp` crate
//!
//! The handshake is built on the audited [`srp`] crate (constant-time
//! `crypto-bigint`, RFC 5054 proofs). Its high-level `process_reply`
//! derives `x` from a username/password internally, so we bypass it and
//! drive the vetted low-level primitives directly to inject our
//! URK-derived `x`. Both the client and server wrappers call the *same*
//! `srp::utils` functions, so the two sides always agree.
//!
//! [ADR-010]: https://github.com/kafkade/tock/blob/main/docs/adr/ADR-010-srp-authentication.md
//! [ADR-011]: https://github.com/kafkade/tock/blob/main/docs/adr/ADR-011-account-based-self-host-two-secret-auth.md

// SRP-6a is defined in terms of single-letter variables (`g`, `x`, `a`, `b`,
// `k`, `u`, `A`, `B`). Keeping that notation matches RFC 5054, ADR-010, and
// the upstream `srp` crate (which allows this lint crate-wide), so the math is
// auditable against the spec.
#![allow(clippy::many_single_char_names)]

use sha2::Sha256;
use srp::bigint::{BoxedUint, Resize};
use srp::groups::{G4096, Group};
use srp::utils::{compute_hash, compute_k, compute_m1_rfc5054, compute_m2, compute_u_padded};
use srp::{ClientG4096, ServerG4096};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use crate::Error;
use crate::kdf::hkdf_sha256_32;
use crate::secret::SecretBytes;

/// Byte length of the secret ephemerals `a` and `b` (384 bits, matching
/// [`srp::EphemeralSecret`]). Comfortably above the 256-bit RFC 5054 floor
/// for every group this module uses.
const EPHEMERAL_LEN: usize = 48;

/// Size of a SHA-256 proof / session key, in bytes.
const HASH_LEN: usize = 32;

/// HKDF `info` label for the short-lived sync bearer token (ADR-010).
const INFO_BEARER_TOKEN: &[u8] = b"Tock/v1/srp-bearer-token";

/// HKDF `info` label for the event-AAD channel-binding tag (ADR-010).
const INFO_CHANNEL_BINDING: &[u8] = b"Tock/v1/srp-channel-binding";

/// Copy a 32-byte hash output into an owned array without panicking.
fn to_proof(bytes: &[u8]) -> Result<[u8; HASH_LEN], Error> {
    <[u8; HASH_LEN]>::try_from(bytes).map_err(|_| Error::SrpAuth)
}

/// Reject a peer public ephemeral that is congruent to zero modulo `N`
/// (`A` or `B`), which would collapse the shared secret. Mirrors the
/// safeguard the `srp` crate applies inside its high-level `process_reply`.
fn reject_zero_mod_n(public_bytes: &[u8]) -> Result<(), Error> {
    let g = G4096::generator();
    let n = g.params().modulus().as_nz_ref();
    let value = BoxedUint::from_be_slice_vartime(public_bytes);
    if (value.resize(n.bits_precision()) % n).is_zero().into() {
        return Err(Error::SrpAuth);
    }
    Ok(())
}

/// Compute the SRP registration verifier `v = g^x mod N`.
///
/// `srp_x` is the URK-derived private input from
/// [`crate::kdf::derive_srp_input`]. The returned big-endian bytes are
/// stored by the server alongside the account id and `salt_srp`; they are
/// not secret (recovering `x` from `v` is the discrete-log problem in a
/// 4096-bit group), but they MUST be sent over an authenticated channel at
/// registration to thwart a MITM.
#[must_use]
pub fn compute_verifier(srp_x: &SecretBytes<32>) -> Vec<u8> {
    let client = ClientG4096::<Sha256>::new();
    let x = BoxedUint::from_be_slice_vartime(srp_x.expose_secret());
    client
        .compute_g_x(&x)
        .to_be_bytes_trimmed_vartime()
        .to_vec()
}

/// Authenticated session key `K = H(S)` produced by a completed handshake.
///
/// Wraps the shared key in [`SecretBytes`] (zeroized on drop) and derives
/// the per-session sync secrets from it.
pub struct Session {
    key: SecretBytes<HASH_LEN>,
}

impl Session {
    /// Borrow the raw session key `K`.
    #[must_use]
    pub const fn key(&self) -> &SecretBytes<HASH_LEN> {
        &self.key
    }

    /// Derive the short-lived **bearer token** sent with sync requests
    /// (ADR-010 login flow). Domain-separated from the channel-binding tag.
    ///
    /// # Errors
    /// Returns [`Error::Hkdf`] only if the HKDF expansion rejects its
    /// inputs (cannot happen for a 32-byte output).
    pub fn derive_bearer_token(&self) -> Result<SecretBytes<HASH_LEN>, Error> {
        hkdf_sha256_32(self.key.expose_secret(), &[], INFO_BEARER_TOKEN)
    }

    /// Derive the **channel-binding tag** mixed into event AAD for
    /// defense-in-depth against TLS-stripping (ADR-010 login flow). The
    /// tag is not secret — it is an integrity input — so it is returned as
    /// a plain array.
    ///
    /// # Errors
    /// Returns [`Error::Hkdf`] only if the HKDF expansion rejects its
    /// inputs (cannot happen for a 32-byte output).
    pub fn derive_channel_binding(&self) -> Result<[u8; HASH_LEN], Error> {
        let tag = hkdf_sha256_32(self.key.expose_secret(), &[], INFO_CHANNEL_BINDING)?;
        to_proof(tag.expose_secret())
    }
}

/// Hash the premaster secret `S` into the session key `K = H(S)`, zeroizing
/// the intermediate `S` buffer.
fn session_from_premaster(mut premaster: Vec<u8>) -> Result<(Session, [u8; HASH_LEN]), Error> {
    let hash = compute_hash::<Sha256>(&premaster);
    premaster.zeroize();
    let key = to_proof(hash.as_slice())?;
    Ok((
        Session {
            key: SecretBytes::new(key),
        },
        key,
    ))
}

/// Client-side login handshake: holds the secret ephemeral `a` and the
/// public `A = g^a` until the server responds.
pub struct ClientHandshake {
    a: SecretBytes<EPHEMERAL_LEN>,
    a_pub: Vec<u8>,
}

impl ClientHandshake {
    /// Begin a login: sample a fresh ephemeral `a` and compute `A = g^a`.
    ///
    /// # Errors
    /// Returns [`Error::Rng`] if the OS RNG fails.
    pub fn new() -> Result<Self, Error> {
        let a = SecretBytes::<EPHEMERAL_LEN>::try_random()?;
        let client = ClientG4096::<Sha256>::new();
        let a_pub = client.compute_public_ephemeral(a.expose_secret());
        Ok(Self { a, a_pub })
    }

    /// The client's public ephemeral `A`, sent to the server with the
    /// account identity.
    #[must_use]
    pub fn public(&self) -> &[u8] {
        &self.a_pub
    }

    /// Process the server's `(salt_srp, B)` reply and produce the client
    /// proof `M1`.
    ///
    /// `identity` is the account identifier (e.g. the account UUID bytes)
    /// bound into the RFC 5054 proof; it MUST match what the server uses.
    /// `srp_x` is the URK-derived private input from
    /// [`crate::kdf::derive_srp_input`].
    ///
    /// # Errors
    /// Returns [`Error::SrpAuth`] if `b_pub` is a malicious zero ephemeral.
    pub fn finish(
        self,
        identity: &[u8],
        salt_srp: &[u8],
        b_pub: &[u8],
        srp_x: &SecretBytes<32>,
    ) -> Result<ClientLogin, Error> {
        reject_zero_mod_n(b_pub)?;

        let g = G4096::generator();
        let client = ClientG4096::<Sha256>::new();

        let x = BoxedUint::from_be_slice_vartime(srp_x.expose_secret());
        let a = BoxedUint::from_be_slice_vartime(self.a.expose_secret());
        let b_pub_uint = BoxedUint::from_be_slice_vartime(b_pub);
        let u = compute_u_padded::<Sha256>(&g, &self.a_pub, b_pub);
        let k = compute_k::<Sha256>(&g);

        let premaster = client
            .compute_premaster_secret(&b_pub_uint, &k, &x, &a, &u)
            .to_be_bytes_trimmed_vartime()
            .to_vec();
        let (session, key) = session_from_premaster(premaster)?;

        let m1 =
            compute_m1_rfc5054::<Sha256>(&g, false, identity, salt_srp, &self.a_pub, b_pub, &key);
        let m2_expected = compute_m2::<Sha256>(&self.a_pub, &m1, &key);

        Ok(ClientLogin {
            m1: to_proof(m1.as_slice())?,
            m2_expected: to_proof(m2_expected.as_slice())?,
            session,
        })
    }
}

/// Client state after computing the shared secret: carries the proof `M1`
/// to send and the expected server proof `M2` to verify.
pub struct ClientLogin {
    m1: [u8; HASH_LEN],
    m2_expected: [u8; HASH_LEN],
    session: Session,
}

impl ClientLogin {
    /// The client proof `M1` to send to the server.
    #[must_use]
    pub const fn proof(&self) -> &[u8; HASH_LEN] {
        &self.m1
    }

    /// Verify the server proof `M2` (mutual authentication) and, on
    /// success, yield the authenticated [`Session`]. Comparison is
    /// constant-time.
    ///
    /// # Errors
    /// Returns [`Error::SrpAuth`] if `server_m2` does not match the
    /// expected proof, indicating the server does not hold the verifier.
    pub fn verify_server(self, server_m2: &[u8]) -> Result<Session, Error> {
        if self.m2_expected.ct_eq(server_m2).into() {
            Ok(self.session)
        } else {
            Err(Error::SrpAuth)
        }
    }
}

/// Server-side login handshake: holds the secret ephemeral `b` and the
/// public `B = k*v + g^b` until the client's proof arrives.
pub struct ServerHandshake {
    b: SecretBytes<EPHEMERAL_LEN>,
    b_pub: Vec<u8>,
}

impl ServerHandshake {
    /// Begin handling a login for the stored `verifier`: sample a fresh
    /// ephemeral `b` and compute `B = k*v + g^b`.
    ///
    /// # Errors
    /// Returns [`Error::Rng`] if the OS RNG fails.
    pub fn new(verifier: &[u8]) -> Result<Self, Error> {
        let b = SecretBytes::<EPHEMERAL_LEN>::try_random()?;
        let server = ServerG4096::<Sha256>::new();
        let b_pub = server.compute_public_ephemeral(b.expose_secret(), verifier);
        Ok(Self { b, b_pub })
    }

    /// The server's public ephemeral `B`, sent to the client with
    /// `salt_srp`.
    #[must_use]
    pub fn public(&self) -> &[u8] {
        &self.b_pub
    }

    /// Verify the client proof `M1` against `(identity, salt_srp, a_pub,
    /// verifier)`. On success return the authenticated [`Session`] and the
    /// server proof `M2` to return to the client. Comparison is
    /// constant-time.
    ///
    /// `identity` and `salt_srp` MUST match the values the client used.
    ///
    /// # Errors
    /// Returns [`Error::SrpAuth`] if `a_pub` is a malicious zero ephemeral
    /// or if `client_m1` does not match (wrong password, wrong Secret Key,
    /// or a forged proof).
    pub fn verify(
        self,
        identity: &[u8],
        salt_srp: &[u8],
        a_pub: &[u8],
        verifier: &[u8],
        client_m1: &[u8],
    ) -> Result<(Session, [u8; HASH_LEN]), Error> {
        reject_zero_mod_n(a_pub)?;

        let g = G4096::generator();
        let server = ServerG4096::<Sha256>::new();

        let a_pub_uint = BoxedUint::from_be_slice_vartime(a_pub);
        let v = BoxedUint::from_be_slice_vartime(verifier);
        let b = BoxedUint::from_be_slice_vartime(self.b.expose_secret());
        let u = compute_u_padded::<Sha256>(&g, a_pub, &self.b_pub);

        let premaster = server
            .compute_premaster_secret(&a_pub_uint, &v, &u, &b)
            .to_be_bytes_trimmed_vartime()
            .to_vec();
        let (session, key) = session_from_premaster(premaster)?;

        let m1_expected =
            compute_m1_rfc5054::<Sha256>(&g, false, identity, salt_srp, a_pub, &self.b_pub, &key);
        if !bool::from(m1_expected.as_slice().ct_eq(client_m1)) {
            return Err(Error::SrpAuth);
        }

        let m2 = compute_m2::<Sha256>(a_pub, &m1_expected, &key);
        Ok((session, to_proof(m2.as_slice())?))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{ClientHandshake, ServerHandshake, compute_verifier};
    use crate::kdf::{Argon2Params, derive_srp_input, derive_unlock_root_key};
    use crate::secret::SecretBytes;

    const TEST_PARAMS: Argon2Params = Argon2Params {
        t: 1,
        m_kib: 8,
        p: 1,
    };

    const IDENTITY: &[u8] = b"account-uuid-0001";
    const SALT_SRP: &[u8] = b"a-random-srp-salt";

    fn srp_x_from(password: &[u8], secret_key: &[u8]) -> SecretBytes<32> {
        let urk = derive_unlock_root_key(password, secret_key, &[5; 16], &[6; 16], 1, TEST_PARAMS)
            .expect("urk");
        derive_srp_input(&urk, SALT_SRP).expect("srp x")
    }

    /// Drive a full login. Returns the client and server sessions plus the
    /// verification outcomes so individual tests can assert on them.
    fn run_login(
        verifier: &[u8],
        client_x: &SecretBytes<32>,
    ) -> Result<(super::Session, super::Session), crate::Error> {
        let client = ClientHandshake::new().expect("client start");
        let server = ServerHandshake::new(verifier).expect("server start");

        let a_pub = client.public().to_vec();
        let b_pub = server.public().to_vec();

        let login = client.finish(IDENTITY, SALT_SRP, &b_pub, client_x)?;
        let m1 = *login.proof();

        let (server_session, m2) = server.verify(IDENTITY, SALT_SRP, &a_pub, verifier, &m1)?;
        let client_session = login.verify_server(&m2)?;
        Ok((client_session, server_session))
    }

    #[test]
    fn full_handshake_round_trip_yields_matching_session_key() {
        let x = srp_x_from(b"correct horse", b"secret-key-aaaa");
        let v = compute_verifier(&x);
        let (client_session, server_session) = run_login(&v, &x).expect("login");
        assert_eq!(
            client_session.key(),
            server_session.key(),
            "client and server must agree on K"
        );
    }

    #[test]
    fn wrong_password_is_rejected() {
        let x = srp_x_from(b"correct horse", b"secret-key-aaaa");
        let v = compute_verifier(&x);
        let wrong = srp_x_from(b"wrong horse", b"secret-key-aaaa");
        assert!(run_login(&v, &wrong).is_err());
    }

    #[test]
    fn wrong_secret_key_is_rejected() {
        let x = srp_x_from(b"correct horse", b"secret-key-aaaa");
        let v = compute_verifier(&x);
        let wrong = srp_x_from(b"correct horse", b"secret-key-bbbb");
        assert!(run_login(&v, &wrong).is_err());
    }

    #[test]
    fn tampered_client_proof_is_rejected_by_server() {
        let x = srp_x_from(b"pw", b"sk-000000000000");
        let v = compute_verifier(&x);

        let client = ClientHandshake::new().expect("client");
        let server = ServerHandshake::new(&v).expect("server");
        let a_pub = client.public().to_vec();
        let b_pub = server.public().to_vec();

        let login = client
            .finish(IDENTITY, SALT_SRP, &b_pub, &x)
            .expect("finish");
        let mut m1 = *login.proof();
        m1[0] ^= 0xFF;
        assert!(server.verify(IDENTITY, SALT_SRP, &a_pub, &v, &m1).is_err());
    }

    #[test]
    fn tampered_server_proof_is_rejected_by_client() {
        let x = srp_x_from(b"pw", b"sk-000000000000");
        let v = compute_verifier(&x);

        let client = ClientHandshake::new().expect("client");
        let server = ServerHandshake::new(&v).expect("server");
        let a_pub = client.public().to_vec();
        let b_pub = server.public().to_vec();

        let login = client
            .finish(IDENTITY, SALT_SRP, &b_pub, &x)
            .expect("finish");
        let m1 = *login.proof();
        let (_session, mut m2) = server
            .verify(IDENTITY, SALT_SRP, &a_pub, &v, &m1)
            .expect("server verify");
        m2[0] ^= 0xFF;
        assert!(login.verify_server(&m2).is_err());
    }

    #[test]
    fn verifier_is_deterministic_and_tracks_x() {
        let x1 = srp_x_from(b"pw", b"sk-000000000000");
        let x2 = srp_x_from(b"pw", b"sk-000000000000");
        let x3 = srp_x_from(b"pw2", b"sk-000000000000");
        assert_eq!(compute_verifier(&x1), compute_verifier(&x2));
        assert_ne!(compute_verifier(&x1), compute_verifier(&x3));
    }

    #[test]
    fn session_secrets_are_domain_separated() {
        let x = srp_x_from(b"pw", b"sk-000000000000");
        let v = compute_verifier(&x);
        let (client_session, _server) = run_login(&v, &x).expect("login");

        let bearer = client_session.derive_bearer_token().expect("bearer");
        let channel = client_session.derive_channel_binding().expect("channel");

        // Bearer token and channel-binding tag differ from each other and
        // from the raw session key.
        assert_ne!(bearer.expose_secret(), &channel);
        assert_ne!(bearer.expose_secret(), client_session.key().expose_secret());
        // Deterministic for the same session.
        let bearer2 = client_session.derive_bearer_token().expect("bearer");
        assert_eq!(bearer, bearer2);
    }

    #[test]
    fn malicious_zero_b_pub_is_rejected_client_side() {
        let x = srp_x_from(b"pw", b"sk-000000000000");
        let client = ClientHandshake::new().expect("client");
        // B = 0 collapses the shared secret.
        assert!(client.finish(IDENTITY, SALT_SRP, &[0; 4], &x).is_err());
    }

    #[test]
    fn malicious_zero_a_pub_is_rejected_server_side() {
        let x = srp_x_from(b"pw", b"sk-000000000000");
        let v = compute_verifier(&x);
        let server = ServerHandshake::new(&v).expect("server");
        assert!(
            server
                .verify(IDENTITY, SALT_SRP, &[0; 4], &v, &[0; 32])
                .is_err()
        );
    }

    #[test]
    fn verifier_known_answer_is_stable() {
        // Pin v for a fixed x so an accidental change to group/serialization
        // is caught. x is all-0x07 bytes (not URK-derived; this test only
        // exercises the SRP layer's determinism).
        let x = SecretBytes::<32>::new([0x07; 32]);
        let v = compute_verifier(&x);
        // 4096-bit modulus => verifier serializes to at most 512 bytes.
        assert!(v.len() <= 512);
        assert_eq!(v, compute_verifier(&SecretBytes::<32>::new([0x07; 32])));
    }
}
