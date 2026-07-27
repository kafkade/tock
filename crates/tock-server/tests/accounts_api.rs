//! End-to-end HTTP acceptance test for the self-hosted account system
//! (issue #127). It boots a real `tock-server` on an ephemeral port — so the
//! service is built *with* connect info, exercising the real extractors and the
//! `require_admin` guard — and drives the registration + admin endpoints over
//! HTTP, asserting the acceptance criteria:
//!
//! 1. the first registration on a fresh instance is bootstrapped as an admin
//!    and yields an interim admin bearer token (Immich first-run pattern);
//! 2. the admin API is rejected without that token and accepted with it;
//! 3. under the default `disabled` policy a user can only register with an
//!    admin-minted invite;
//! 4. the registration policy can be read and changed through the admin API;
//! 5. every stored account carries an SRP verifier — never a plaintext
//!    password or Secret Key (the server only ever receives base64 verifier
//!    material here).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::net::SocketAddr;
use std::path::PathBuf;

use tock_server::ServerMode;

use time::OffsetDateTime;
use tock_core::vault::{
    Argon2HeaderParams, FORMAT_VERSION, MAGIC, MIN_COMPAT_VERSION, VaultHeader,
};
use uuid::Uuid;

/// A `tock-server` running on a background thread bound to an ephemeral port.
struct TestServer {
    base_url: String,
    _tmp: tempfile::TempDir,
}

impl TestServer {
    fn start() -> Self {
        let tmp = tempfile::tempdir().expect("server tmp dir");
        let data_dir: PathBuf = tmp.path().to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel::<SocketAddr>();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("server runtime");
            rt.block_on(async move {
                let state = tock_server::open_app_state(&data_dir, ServerMode::SelfHosted)
                    .expect("open server state");
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind ephemeral port");
                let addr = listener.local_addr().expect("local addr");
                tx.send(addr).expect("send addr");
                tock_server::serve(listener, state).await.expect("serve");
            });
        });

        let addr = rx.recv().expect("server failed to bind");
        Self {
            base_url: format!("http://{addr}"),
            _tmp: tmp,
        }
    }
}

/// Minimal standard-alphabet base64 encoder, so the test doesn't pull in a
/// base64 dependency just to format opaque verifier bytes.
fn b64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map_or(0, |b| u32::from(*b));
        let b2 = chunk.get(2).map_or(0, |b| u32::from(*b));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn register_body(username: &str, invite: Option<&str>) -> serde_json::Value {
    let mut body = serde_json::json!({
        "username": username,
        "srp_salt": b64(&[0xAA; 16]),
        "srp_verifier": b64(&[0xBB; 64]),
        "srp_group": "RFC5054-4096-SHA256",
        "kdf_params": { "alg": "argon2id", "t": 3, "m": 65536, "p": 1 },
    });
    if let Some(token) = invite {
        body["invite_token"] = serde_json::json!(token);
    }
    body
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn account_system_end_to_end() {
    let server = TestServer::start();
    let http = reqwest::Client::new();
    let base = &server.base_url;

    // 1. First registration bootstraps an admin + mints an admin token.
    let resp = http
        .post(format!("{base}/v1/accounts/register"))
        .json(&register_body("admin", None))
        .send()
        .await
        .expect("register admin");
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let admin: serde_json::Value = resp.json().await.expect("admin json");
    assert_eq!(admin["role"], "admin");
    assert_eq!(admin["status"], "active");
    let admin_token = admin["admin_token"]
        .as_str()
        .expect("admin token")
        .to_string();
    assert!(admin_token.starts_with("adm_"));

    // 2. Admin API is rejected without a token.
    let resp = http
        .get(format!("{base}/v1/admin/users"))
        .send()
        .await
        .expect("list without token");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    // ...and with a bogus token.
    let resp = http
        .get(format!("{base}/v1/admin/users"))
        .bearer_auth("adm_not-a-real-token")
        .send()
        .await
        .expect("list bogus token");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    // 3. Authorized: the admin sees itself in the user list.
    let resp = http
        .get(format!("{base}/v1/admin/users"))
        .bearer_auth(&admin_token)
        .send()
        .await
        .expect("list users");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let users: serde_json::Value = resp.json().await.expect("users json");
    assert_eq!(users.as_array().expect("array").len(), 1);
    assert_eq!(users[0]["username"], "admin");
    assert_eq!(users[0]["role"], "admin");

    // 4. Default policy is `disabled`: self-registration without an invite fails.
    let resp = http
        .post(format!("{base}/v1/accounts/register"))
        .json(&register_body("bob", None))
        .send()
        .await
        .expect("register bob no invite");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    // 5. Admin mints an invite; the user redeems it with their own SRP creds.
    let resp = http
        .post(format!("{base}/v1/admin/users"))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({ "username": "bob" }))
        .send()
        .await
        .expect("mint invite");
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let invite: serde_json::Value = resp.json().await.expect("invite json");
    let invite_token = invite["invite_token"]
        .as_str()
        .expect("invite token")
        .to_string();

    let resp = http
        .post(format!("{base}/v1/accounts/register"))
        .json(&register_body("bob", Some(&invite_token)))
        .send()
        .await
        .expect("register bob with invite");
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let bob: serde_json::Value = resp.json().await.expect("bob json");
    assert_eq!(bob["role"], "user");
    assert!(bob.get("admin_token").is_none());

    // The invite is single-use.
    let resp = http
        .post(format!("{base}/v1/accounts/register"))
        .json(&register_body("carol", Some(&invite_token)))
        .send()
        .await
        .expect("reuse invite");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    // 6. Admin flips the policy to `open`; self-registration now succeeds.
    let resp = http
        .put(format!("{base}/v1/admin/settings"))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({ "registration_policy": "open" }))
        .send()
        .await
        .expect("set policy open");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = http
        .get(format!("{base}/v1/admin/settings"))
        .bearer_auth(&admin_token)
        .send()
        .await
        .expect("get settings");
    let settings: serde_json::Value = resp.json().await.expect("settings json");
    assert_eq!(settings["registration_policy"], "open");

    let resp = http
        .post(format!("{base}/v1/accounts/register"))
        .json(&register_body("dave", None))
        .send()
        .await
        .expect("open self-register");
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let dave: serde_json::Value = resp.json().await.expect("dave json");
    assert_eq!(dave["role"], "user");

    // 7. Hosted billing endpoints stay gated off in self-hosted mode.
    let resp = http
        .post(format!("{base}/v1/accounts"))
        .json(&serde_json::json!({ "email": "x@example.com" }))
        .send()
        .await
        .expect("hosted create");
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

/// Serialize a minimal, well-formed public vault header embedding `vault_id`.
fn header_bytes(vault_id: Uuid) -> Vec<u8> {
    VaultHeader {
        magic: MAGIC,
        format_version: FORMAT_VERSION,
        min_compatible_version: MIN_COMPAT_VERSION,
        vault_id,
        account_id: Uuid::from_bytes([8; 16]),
        kdf_version: 1,
        kdf_salt: [9; 16],
        hkdf_salt: [3; 32],
        argon2: Argon2HeaderParams {
            t: 3,
            m_kib: 65_536,
            p: 1,
        },
        vk_wrap_nonce: [5; 12],
        vk_wrap_ct: vec![0xAB; 48],
        created_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("ts"),
        storage_layout: "sqlite-plain-app-aead-v0".to_string(),
    }
    .to_bytes()
}

fn hex16(bytes: [u8; 16]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(32), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

fn register_body_with_vault(
    username: &str,
    vault_id_hex: &str,
    header_b64: &str,
) -> serde_json::Value {
    let mut body = register_body(username, None);
    body["vault_id"] = serde_json::json!(vault_id_hex);
    body["header"] = serde_json::json!(header_b64);
    body
}

/// AC #2 (issue #199): registration parses the uploaded header and cross-checks
/// the submitted `vault_id` against the header's embedded `vault_id`, rejecting a
/// mismatch before storing anything (nothing consumed), and accepting a match.
#[tokio::test]
async fn register_validates_vault_id_matches_header() {
    let server = TestServer::start();
    let base = &server.base_url;
    let http = reqwest::Client::new();

    let submitted = [0x33_u8; 16];
    // Header embeds a DIFFERENT vault_id than the one submitted alongside it.
    let mismatched = b64(&header_bytes(Uuid::from_bytes([0x44; 16])));
    let resp = http
        .post(format!("{base}/v1/accounts/register"))
        .json(&register_body_with_vault(
            "alice",
            &hex16(submitted),
            &mismatched,
        ))
        .send()
        .await
        .expect("register mismatched header");
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    // A malformed (non-header) blob is likewise rejected.
    let resp = http
        .post(format!("{base}/v1/accounts/register"))
        .json(&register_body_with_vault(
            "alice",
            &hex16(submitted),
            &b64(b"not-a-real-header"),
        ))
        .send()
        .await
        .expect("register malformed header");
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    // Nothing was consumed by the rejected attempts: the same username still
    // registers with a MATCHING header and is accepted (bootstrap admin).
    let matching = b64(&header_bytes(Uuid::from_bytes(submitted)));
    let resp = http
        .post(format!("{base}/v1/accounts/register"))
        .json(&register_body_with_vault(
            "alice",
            &hex16(submitted),
            &matching,
        ))
        .send()
        .await
        .expect("register matching header");
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
}
