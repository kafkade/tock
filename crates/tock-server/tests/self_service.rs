//! End-to-end HTTP acceptance tests for the browser self-service portal and
//! admin instance settings/stats (issue #131). Boots a real `tock-server` and
//! drives the client side with the real `tock_crypto` SRP + KDF primitives,
//! asserting:
//!
//! 1. password rotation replaces the SRP verifier and re-wrapped vault header —
//!    the new password logs in, the old one is rejected, and the stored header
//!    is updated byte-for-byte;
//! 2. a user can list their own sessions (with the current one flagged) and
//!    revoke others / a specific one;
//! 3. a user can list and revoke their own devices;
//! 4. all self-service routes reject anonymous callers (401);
//! 5. the admin settings carry a public address and `GET /v1/admin/stats`
//!    reports non-secret instance counters.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
#![allow(clippy::too_many_lines)]

use std::net::SocketAddr;
use std::path::PathBuf;

use tock_crypto::kdf::{Argon2Params, derive_srp_input, derive_unlock_root_key};
use tock_crypto::secret::SecretBytes;
use tock_crypto::srp::{ClientHandshake, compute_verifier};
use tock_server::ServerMode;

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

// ── Encoding helpers (dependency-free, mirroring the server codec) ───────

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

fn b64_decode(s: &str) -> Vec<u8> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for ch in s.bytes() {
        let val = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

const TEST_PARAMS: Argon2Params = Argon2Params {
    t: 1,
    m_kib: 8,
    p: 1,
};

/// A client account holding the inputs needed to register, log in, and rotate.
struct Account {
    username: String,
    salt_srp: Vec<u8>,
    srp_x: SecretBytes<32>,
    verifier: Vec<u8>,
}

impl Account {
    fn new(username: &str, password: &[u8], secret_key: &[u8]) -> Self {
        let salt_srp = vec![0x5A; 16];
        let urk = derive_unlock_root_key(
            password,
            secret_key,
            &[0x11; 16],
            &[0x22; 16],
            1,
            TEST_PARAMS,
        )
        .expect("urk");
        let srp_x = derive_srp_input(&urk, &salt_srp).expect("srp x");
        let verifier = compute_verifier(&srp_x);
        Self {
            username: username.to_string(),
            salt_srp,
            srp_x,
            verifier,
        }
    }

    fn register_body(&self, invite: Option<&str>) -> serde_json::Value {
        let mut body = serde_json::json!({
            "username": self.username,
            "srp_salt": b64(&self.salt_srp),
            "srp_verifier": b64(&self.verifier),
            "srp_group": "RFC5054-4096-SHA256",
            "kdf_params": { "alg": "argon2id", "t": 1, "m": 8, "p": 1 },
        });
        if let Some(token) = invite {
            body["invite_token"] = serde_json::json!(token);
        }
        body
    }

    /// Body for `PUT /v1/account/srp-verifier` rotating to this account's
    /// verifier, optionally carrying a re-wrapped header blob.
    fn rotate_body(&self, header: Option<&[u8]>) -> serde_json::Value {
        let mut body = serde_json::json!({
            "srp_salt": b64(&self.salt_srp),
            "srp_verifier": b64(&self.verifier),
            "srp_group": "RFC5054-4096-SHA256",
            "kdf_params": { "alg": "argon2id", "t": 1, "m": 8, "p": 1 },
        });
        if let Some(header) = header {
            body["header"] = serde_json::json!(b64(header));
        }
        body
    }
}

#[derive(Debug)]
struct Session {
    bearer: String,
}

async fn login(
    http: &reqwest::Client,
    base: &str,
    account: &Account,
) -> Result<Session, reqwest::StatusCode> {
    let client = ClientHandshake::new().expect("client handshake");
    let a_pub = client.public().to_vec();

    let resp = http
        .post(format!("{base}/v1/auth/srp/start"))
        .json(&serde_json::json!({ "username": account.username, "a_pub": b64(&a_pub) }))
        .send()
        .await
        .expect("srp start");
    if !resp.status().is_success() {
        return Err(resp.status());
    }
    let start: serde_json::Value = resp.json().await.expect("start json");
    let handshake_id = start["handshake_id"].as_str().expect("handshake_id");
    let salt = b64_decode(start["salt"].as_str().expect("salt"));
    let b_pub = b64_decode(start["b_pub"].as_str().expect("b_pub"));

    let client_login = client
        .finish(account.username.as_bytes(), &salt, &b_pub, &account.srp_x)
        .expect("client finish");
    let m1 = client_login.proof();

    let resp = http
        .post(format!("{base}/v1/auth/srp/finish"))
        .json(&serde_json::json!({ "handshake_id": handshake_id, "m1": b64(m1) }))
        .send()
        .await
        .expect("srp finish");
    if !resp.status().is_success() {
        return Err(resp.status());
    }
    let finish: serde_json::Value = resp.json().await.expect("finish json");
    let m2 = b64_decode(finish["m2"].as_str().expect("m2"));

    let session = client_login.verify_server(&m2).expect("verify server m2");
    let bearer = session.derive_bearer_token().expect("bearer");
    Ok(Session {
        bearer: hex(bearer.expose_secret()),
    })
}

async fn register(
    http: &reqwest::Client,
    base: &str,
    body: &serde_json::Value,
) -> serde_json::Value {
    let resp = http
        .post(format!("{base}/v1/accounts/register"))
        .json(body)
        .send()
        .await
        .expect("register");
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED, "registration");
    resp.json().await.expect("register json")
}

#[tokio::test]
async fn password_rotation_replaces_verifier_and_header() {
    let server = TestServer::start();
    let http = reqwest::Client::new();
    let base = &server.base_url;
    let vault = hex(&[0xC3; 16]);

    // Alice registers (admin) and logs in with her original password.
    let alice = Account::new("alice", b"old-password", b"alice-secret-key");
    register(&http, base, &alice.register_body(None)).await;
    let sess = login(&http, base, &alice).await.expect("alice login");

    // She uploads an initial vault header (claims the vault for her account).
    let old_header = b"header-wrapped-under-old-mek";
    let resp = http
        .put(format!("{base}/v1/vaults/{vault}/header"))
        .bearer_auth(&sess.bearer)
        .json(&serde_json::json!({ "header": b64(old_header) }))
        .send()
        .await
        .expect("put header");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    // Anonymous rotation is rejected.
    let rotated = Account::new("alice", b"new-password", b"alice-secret-key");
    let new_header = b"header-rewrapped-under-new-mek";
    let resp = http
        .put(format!("{base}/v1/account/srp-verifier"))
        .json(&rotated.rotate_body(Some(new_header)))
        .send()
        .await
        .expect("anon rotate");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Authenticated rotation succeeds and updates the stored header.
    let resp = http
        .put(format!("{base}/v1/account/srp-verifier"))
        .bearer_auth(&sess.bearer)
        .json(&rotated.rotate_body(Some(new_header)))
        .send()
        .await
        .expect("rotate");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    // The old password no longer authenticates.
    let status = login(&http, base, &alice)
        .await
        .expect_err("old password must fail after rotation");
    assert_eq!(status, reqwest::StatusCode::UNAUTHORIZED);

    // The new password logs in, and the stored header is the re-wrapped blob.
    let new_sess = login(&http, base, &rotated)
        .await
        .expect("new password login");
    let resp = http
        .get(format!("{base}/v1/account/header"))
        .bearer_auth(&new_sess.bearer)
        .send()
        .await
        .expect("get header after rotation");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.expect("header json");
    assert_eq!(b64_decode(body["header"].as_str().unwrap()), new_header);
}

#[tokio::test]
async fn sessions_and_devices_self_service() {
    let server = TestServer::start();
    let http = reqwest::Client::new();
    let base = &server.base_url;
    let vault = hex(&[0xD4; 16]);

    let alice = Account::new("alice", b"pw", b"sk");
    register(&http, base, &alice.register_body(None)).await;

    // Anonymous self-service is rejected.
    for resp in [
        http.get(format!("{base}/v1/account/sessions"))
            .send()
            .await
            .expect("anon sessions"),
        http.get(format!("{base}/v1/account/devices"))
            .send()
            .await
            .expect("anon devices"),
    ] {
        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    // Two concurrent logins → two sessions.
    let sess1 = login(&http, base, &alice).await.expect("login 1");
    let sess2 = login(&http, base, &alice).await.expect("login 2");

    // Session 1 lists both sessions and sees itself flagged as current.
    let resp = http
        .get(format!("{base}/v1/account/sessions"))
        .bearer_auth(&sess1.bearer)
        .send()
        .await
        .expect("list sessions");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let sessions: serde_json::Value = resp.json().await.expect("sessions json");
    let arr = sessions.as_array().expect("sessions array");
    assert_eq!(arr.len(), 2, "two live sessions");
    let current_count = arr.iter().filter(|s| s["current"] == true).count();
    assert_eq!(current_count, 1, "exactly one session flagged current");

    // Revoke-others from session 1 ends session 2.
    let resp = http
        .post(format!("{base}/v1/account/sessions/revoke-others"))
        .bearer_auth(&sess1.bearer)
        .send()
        .await
        .expect("revoke others");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let revoked: serde_json::Value = resp.json().await.expect("revoked json");
    assert_eq!(revoked["revoked"], 1);

    // Session 2 can no longer refresh.
    let resp = http
        .post(format!("{base}/v1/auth/refresh"))
        .bearer_auth(&sess2.bearer)
        .send()
        .await
        .expect("refresh revoked");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Register a device, list it, then revoke it.
    let device_id = hex(&[0xE5; 16]);
    let resp = http
        .post(format!("{base}/v1/vaults/{vault}/devices"))
        .bearer_auth(&sess1.bearer)
        .json(&serde_json::json!({
            "device_id": device_id,
            "verifying_key": hex(&[0xF6; 32]),
            "label": "laptop",
        }))
        .send()
        .await
        .expect("register device");
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);

    let resp = http
        .get(format!("{base}/v1/account/devices"))
        .bearer_auth(&sess1.bearer)
        .send()
        .await
        .expect("list devices");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let devices: serde_json::Value = resp.json().await.expect("devices json");
    let arr = devices.as_array().expect("devices array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], device_id);
    assert_eq!(arr[0]["label"], "laptop");
    assert_eq!(arr[0]["revoked"], false);

    let resp = http
        .delete(format!("{base}/v1/account/devices/{device_id}"))
        .bearer_auth(&sess1.bearer)
        .send()
        .await
        .expect("revoke device");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = http
        .get(format!("{base}/v1/account/devices"))
        .bearer_auth(&sess1.bearer)
        .send()
        .await
        .expect("list devices after revoke");
    let devices: serde_json::Value = resp.json().await.expect("devices json");
    assert_eq!(devices.as_array().expect("array")[0]["revoked"], true);
}

#[tokio::test]
async fn admin_settings_public_address_and_stats() {
    let server = TestServer::start();
    let http = reqwest::Client::new();
    let base = &server.base_url;

    let alice = Account::new("alice", b"pw", b"sk");
    let reg = register(&http, base, &alice.register_body(None)).await;
    let admin_token = reg["admin_token"]
        .as_str()
        .expect("admin token")
        .to_string();

    // Anonymous stats is rejected.
    let resp = http
        .get(format!("{base}/v1/admin/stats"))
        .send()
        .await
        .expect("anon stats");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Set a public address alongside the registration policy.
    let resp = http
        .put(format!("{base}/v1/admin/settings"))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({
            "registration_policy": "invite-only",
            "public_address": "https://tock.example.org",
        }))
        .send()
        .await
        .expect("put settings");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let settings: serde_json::Value = resp.json().await.expect("settings json");
    assert_eq!(settings["registration_policy"], "invite-only");
    assert_eq!(settings["public_address"], "https://tock.example.org");

    // It is echoed by the public server info and admin settings GET.
    let resp = http
        .get(format!("{base}/v1/server/info"))
        .send()
        .await
        .expect("server info");
    let info: serde_json::Value = resp.json().await.expect("info json");
    assert_eq!(info["public_address"], "https://tock.example.org");

    let resp = http
        .get(format!("{base}/v1/admin/settings"))
        .bearer_auth(&admin_token)
        .send()
        .await
        .expect("get settings");
    let settings: serde_json::Value = resp.json().await.expect("settings json");
    assert_eq!(settings["public_address"], "https://tock.example.org");

    // Stats report the single (admin) account.
    let resp = http
        .get(format!("{base}/v1/admin/stats"))
        .bearer_auth(&admin_token)
        .send()
        .await
        .expect("stats");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let stats: serde_json::Value = resp.json().await.expect("stats json");
    assert_eq!(stats["accounts_total"], 1);
    assert_eq!(stats["accounts_admin"], 1);
    assert_eq!(stats["accounts_active"], 1);
    assert_eq!(stats["accounts_disabled"], 0);
}
