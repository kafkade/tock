//! End-to-end HTTP acceptance test for SRP login + account-scoped,
//! authenticated sync (issue #130). It boots a real `tock-server` on an
//! ephemeral port and drives the full client side with the real
//! `tock_crypto::srp` + `kdf` primitives, asserting the issue's acceptance
//! criteria:
//!
//! 1. a registered account can run the SRP handshake and log in; the bearer
//!    token is **independently derived from the session key `K`** on both sides
//!    (it is never sent in the handshake bodies);
//! 2. wrong credentials are rejected at `finish` (401);
//! 3. unauthenticated `devices`/`push`/`pull`/`onboarding` are rejected (401);
//! 4. a session can only touch its own account's vault — cross-account is 403;
//! 5. the SRP channel-binding tag is enforced on the event routes;
//! 6. tokens refresh; and a full register → login → device → push → pull
//!    round-trip returns the ciphertext unchanged;
//! 7. an admin's SRP session token authorizes the admin API.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
#![allow(clippy::too_many_lines)]

use std::net::SocketAddr;
use std::path::PathBuf;

use tock_crypto::kdf::{Argon2Params, derive_srp_input, derive_unlock_root_key};
use tock_crypto::secret::SecretBytes;
use tock_crypto::srp::{ClientHandshake, compute_verifier};
use tock_server::ServerMode;

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

// ── Encoding helpers (kept dependency-free, mirroring the server codec) ──

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

// ── Client-side account + login model ────────────────────────────────

const TEST_PARAMS: Argon2Params = Argon2Params {
    t: 1,
    m_kib: 8,
    p: 1,
};

/// A client account holding the inputs needed to register and log in. The
/// `account_id` array below is client-side KDF salt material (per ADR-011),
/// independent of the server-assigned account id.
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
}

/// A logged-in session: the bearer token and channel-binding tag the client
/// derived locally from `K`.
#[derive(Debug)]
struct Session {
    bearer: String,
    channel_binding: String,
}

/// Run the full SRP login handshake against the server. `wrong_x`, when set,
/// substitutes a different private input to model a wrong password / Secret Key
/// and must cause `finish` to fail.
async fn login(
    http: &reqwest::Client,
    base: &str,
    account: &Account,
    wrong_x: Option<&SecretBytes<32>>,
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

    let x = wrong_x.unwrap_or(&account.srp_x);
    let client_login = client
        .finish(account.username.as_bytes(), &salt, &b_pub, x)
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

    // Mutual auth: the client verifies the server proof and only then trusts the
    // session. The bearer token / channel tag are derived from `K` locally —
    // the server never sent them.
    let session = client_login.verify_server(&m2).expect("verify server m2");
    let bearer = session.derive_bearer_token().expect("bearer");
    let channel_binding = session.derive_channel_binding().expect("channel");
    Ok(Session {
        bearer: hex(bearer.expose_secret()),
        channel_binding: hex(&channel_binding),
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

fn device_body() -> (String, serde_json::Value) {
    let device_id = hex(&[0xD1; 16]);
    let body = serde_json::json!({
        "device_id": device_id,
        "verifying_key": hex(&[0xE2; 32]),
        "label": "test-device",
    });
    (device_id, body)
}

#[tokio::test]
async fn srp_login_and_account_scoped_sync() {
    let server = TestServer::start();
    let http = reqwest::Client::new();
    let base = &server.base_url;

    let vault_a = hex(&[0xA0; 16]);

    // ── Account A: first registration bootstraps an admin. ──────────────
    let alice = Account::new("alice", b"alice-password", b"alice-secret-key");
    let reg_a = register(&http, base, &alice.register_body(None)).await;
    assert_eq!(reg_a["role"], "admin");
    let admin_token = reg_a["admin_token"]
        .as_str()
        .expect("admin token")
        .to_string();

    // ── 3. Unauthenticated sync is rejected (401). ──────────────────────
    let (device_id, dev_body) = device_body();
    for resp in [
        http.post(format!("{base}/v1/vaults/{vault_a}/devices"))
            .json(&dev_body)
            .send()
            .await
            .expect("anon devices"),
        http.post(format!("{base}/v1/vaults/{vault_a}/events/push"))
            .json(&serde_json::json!({ "events": [] }))
            .send()
            .await
            .expect("anon push"),
        http.get(format!("{base}/v1/vaults/{vault_a}/events/pull"))
            .send()
            .await
            .expect("anon pull"),
        http.put(format!("{base}/v1/vaults/{vault_a}/onboarding/{device_id}"))
            .json(&serde_json::json!({ "blob": b64(b"x") }))
            .send()
            .await
            .expect("anon onboarding"),
    ] {
        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    // ── 1. Alice logs in (handshake + mutual auth). ─────────────────────
    let sess_a = login(&http, base, &alice, None).await.expect("alice login");

    // ── 2. Wrong credentials are rejected at finish (401). ──────────────
    let wrong = Account::new("alice", b"WRONG-password", b"alice-secret-key");
    let status = login(&http, base, &alice, Some(&wrong.srp_x))
        .await
        .expect_err("wrong creds must fail");
    assert_eq!(status, reqwest::StatusCode::UNAUTHORIZED);

    // Unknown username is rejected uniformly (401).
    let ghost = Account::new("nobody", b"pw", b"sk");
    let status = login(&http, base, &ghost, None)
        .await
        .expect_err("unknown user must fail");
    assert_eq!(status, reqwest::StatusCode::UNAUTHORIZED);

    // ── 7. Alice's SRP session token authorizes the admin API. ──────────
    let resp = http
        .get(format!("{base}/v1/admin/users"))
        .bearer_auth(&sess_a.bearer)
        .send()
        .await
        .expect("admin via srp session");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    // ── 6. Full round-trip: register device → push → pull. ──────────────
    let resp = http
        .post(format!("{base}/v1/vaults/{vault_a}/devices"))
        .bearer_auth(&sess_a.bearer)
        .json(&dev_body)
        .send()
        .await
        .expect("register device");
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);

    let ciphertext = b"opaque-encrypted-event-frame";
    let push_body = serde_json::json!({
        "events": [{
            "event_id": hex(&[0x01; 16]),
            "device_id": device_id,
            "lamport": 1,
            "payload": b64(ciphertext),
        }],
    });

    // ── 5. Channel-binding tag is enforced on the event routes. ─────────
    let resp = http
        .post(format!("{base}/v1/vaults/{vault_a}/events/push"))
        .bearer_auth(&sess_a.bearer)
        .json(&push_body)
        .send()
        .await
        .expect("push without channel binding");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "missing channel-binding tag must be rejected"
    );

    let resp = http
        .post(format!("{base}/v1/vaults/{vault_a}/events/push"))
        .bearer_auth(&sess_a.bearer)
        .header("X-Tock-Channel-Binding", hex(&[0x00; 32]))
        .json(&push_body)
        .send()
        .await
        .expect("push with wrong channel binding");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "mismatched channel-binding tag must be rejected"
    );

    let resp = http
        .post(format!("{base}/v1/vaults/{vault_a}/events/push"))
        .bearer_auth(&sess_a.bearer)
        .header("X-Tock-Channel-Binding", &sess_a.channel_binding)
        .json(&push_body)
        .send()
        .await
        .expect("push");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let pushed: serde_json::Value = resp.json().await.expect("push json");
    assert_eq!(pushed["accepted"], 1);

    let resp = http
        .get(format!("{base}/v1/vaults/{vault_a}/events/pull"))
        .bearer_auth(&sess_a.bearer)
        .header("X-Tock-Channel-Binding", &sess_a.channel_binding)
        .send()
        .await
        .expect("pull");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let pulled: serde_json::Value = resp.json().await.expect("pull json");
    let events = pulled["events"].as_array().expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(
        b64_decode(events[0]["payload"].as_str().unwrap()),
        ciphertext
    );

    // ── 4. Cross-account access is forbidden (403). ─────────────────────
    // Alice (admin) mints an invite; Bob registers and logs in.
    let resp = http
        .post(format!("{base}/v1/admin/users"))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({ "role": "user" }))
        .send()
        .await
        .expect("mint invite");
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let invite: serde_json::Value = resp.json().await.expect("invite json");
    let invite_token = invite["invite_token"].as_str().expect("invite token");

    let bob = Account::new("bob", b"bob-password", b"bob-secret-key");
    register(&http, base, &bob.register_body(Some(invite_token))).await;
    let sess_b = login(&http, base, &bob, None).await.expect("bob login");

    // Bob cannot read Alice's vault.
    let resp = http
        .get(format!("{base}/v1/vaults/{vault_a}/events/pull"))
        .bearer_auth(&sess_b.bearer)
        .header("X-Tock-Channel-Binding", &sess_b.channel_binding)
        .send()
        .await
        .expect("bob pulls alice vault");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    // Bob cannot write Alice's vault.
    let resp = http
        .post(format!("{base}/v1/vaults/{vault_a}/events/push"))
        .bearer_auth(&sess_b.bearer)
        .header("X-Tock-Channel-Binding", &sess_b.channel_binding)
        .json(&push_body)
        .send()
        .await
        .expect("bob pushes alice vault");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    // Bob is not an admin: his SRP session must not authorize the admin API.
    let resp = http
        .get(format!("{base}/v1/admin/users"))
        .bearer_auth(&sess_b.bearer)
        .send()
        .await
        .expect("bob admin attempt");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    // ── Refresh: a live session refreshes; a bogus token does not. ──────
    let resp = http
        .post(format!("{base}/v1/auth/refresh"))
        .bearer_auth(&sess_a.bearer)
        .send()
        .await
        .expect("refresh");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = http
        .post(format!("{base}/v1/auth/refresh"))
        .bearer_auth(hex(&[0xFF; 32]))
        .send()
        .await
        .expect("refresh bogus");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
