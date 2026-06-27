//! End-to-end 2SKD → SRP-6a integration (ADR-010 / ADR-011 acceptance).
//!
//! Exercises the full public path a client and self-host server take:
//! derive the Unlock Root Key from `(password, Secret Key)`, derive the SRP
//! private input, register a verifier, and run a mutual-auth login. Proves
//! the acceptance criteria: login succeeds with the right secrets and fails
//! if *either* the password or the Secret Key is wrong — without the server
//! ever seeing the password, Secret Key, or URK.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use tock_crypto::kdf::{Argon2Params, derive_srp_input, derive_unlock_root_key};
use tock_crypto::secret::SecretBytes;
use tock_crypto::srp::{ClientHandshake, ServerHandshake, compute_verifier};

// Cheap Argon2id params: this test is about protocol wiring, not KDF cost.
const PARAMS: Argon2Params = Argon2Params {
    t: 1,
    m_kib: 8,
    p: 1,
};

const KDF_SALT: [u8; 16] = [0x11; 16];
const ACCOUNT_ID: [u8; 16] = [0x22; 16];
const KDF_VERSION: u16 = 1;
const SALT_SRP: &[u8] = b"independent-srp-salt";
const IDENTITY: &[u8] = &ACCOUNT_ID;

fn srp_x(password: &[u8], secret_key: &[u8]) -> SecretBytes<32> {
    let urk = derive_unlock_root_key(
        password,
        secret_key,
        &KDF_SALT,
        &ACCOUNT_ID,
        KDF_VERSION,
        PARAMS,
    )
    .expect("urk");
    derive_srp_input(&urk, SALT_SRP).expect("srp x")
}

/// Run a login against `verifier` using the login-time private input
/// `login_x`. Returns whether mutual auth fully succeeded.
fn login_succeeds(verifier: &[u8], login_x: &SecretBytes<32>) -> bool {
    let client = ClientHandshake::new().expect("client start");
    let server = ServerHandshake::new(verifier).expect("server start");

    let a_pub = client.public().to_vec();
    let b_pub = server.public().to_vec();

    let Ok(login) = client.finish(IDENTITY, SALT_SRP, &b_pub, login_x) else {
        return false;
    };
    let m1 = *login.proof();

    let Ok((_server_session, m2)) = server.verify(IDENTITY, SALT_SRP, &a_pub, verifier, &m1) else {
        return false;
    };
    login.verify_server(&m2).is_ok()
}

#[test]
fn register_then_login_with_correct_secrets_succeeds() {
    let password = b"correct horse battery staple";
    let secret_key = b"A4-secret-key-128bit";

    // Registration: server stores only (account_id, salt_srp, verifier).
    let reg_x = srp_x(password, secret_key);
    let verifier = compute_verifier(&reg_x);

    // Login from a fresh device re-deriving x from the same two secrets.
    let login_x = srp_x(password, secret_key);
    assert!(login_succeeds(&verifier, &login_x));
}

#[test]
fn login_fails_with_wrong_password() {
    let secret_key = b"A4-secret-key-128bit";
    let verifier = compute_verifier(&srp_x(b"the-right-password", secret_key));
    let login_x = srp_x(b"the-wrong-password", secret_key);
    assert!(!login_succeeds(&verifier, &login_x));
}

#[test]
fn login_fails_with_wrong_secret_key() {
    let password = b"shared-password";
    let verifier = compute_verifier(&srp_x(password, b"A4-right-secret-key0"));
    let login_x = srp_x(password, b"A4-wrong-secret-key0");
    assert!(!login_succeeds(&verifier, &login_x));
}

#[test]
fn session_secrets_match_across_client_and_server() {
    let password = b"pw";
    let secret_key = b"sk-0123456789abcdef";
    let verifier = compute_verifier(&srp_x(password, secret_key));
    let login_x = srp_x(password, secret_key);

    let client = ClientHandshake::new().expect("client");
    let server = ServerHandshake::new(&verifier).expect("server");
    let a_pub = client.public().to_vec();
    let b_pub = server.public().to_vec();

    let login = client
        .finish(IDENTITY, SALT_SRP, &b_pub, &login_x)
        .expect("finish");
    let m1 = *login.proof();
    let (server_session, m2) = server
        .verify(IDENTITY, SALT_SRP, &a_pub, &verifier, &m1)
        .expect("server verify");
    let client_session = login.verify_server(&m2).expect("client verify");

    // Both sides derive identical bearer tokens and channel-binding tags
    // from the shared session key.
    let cb = client_session.derive_bearer_token().expect("c bearer");
    let sb = server_session.derive_bearer_token().expect("s bearer");
    assert_eq!(cb, sb);
    assert_eq!(
        client_session.derive_channel_binding().expect("c cb"),
        server_session.derive_channel_binding().expect("s cb"),
    );
}
