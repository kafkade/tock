//! End-to-end HTTP acceptance tests for issue #201: IDOR-safe ciphertext
//! **export** and **import** (round-trip / restore). Boots a real `tock-server`
//! on an ephemeral port and drives the client side with the real
//! `tock_crypto::srp` + `kdf` primitives, asserting:
//!
//! 1. an owner can export their vault's ciphertext (non-secret header + full
//!    event log), byte-for-byte, and the server never decrypts;
//! 2. export runs the double-auth pattern — a session for account A can never
//!    export account B's vault (403), anonymous is 401, a disabled account is
//!    denied, and the SRP channel-binding tag is enforced;
//! 3. export is ownership-scoped, not admin-gated — a plain user exports their
//!    own vault;
//! 4. an exported archive imports back into a fresh vault (round-trip that
//!    unblocks cross-server migration #202), is idempotent against duplicate
//!    events, and preserves the header;
//! 5. import enforces the same authorization (anonymous 401, cross-account 403,
//!    channel-binding required).

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

// ── Encoding helpers (dependency-free, mirroring the server codec) ───

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

// ── Client-side account + login model (mirrors srp_sync.rs) ──────────

const TEST_PARAMS: Argon2Params = Argon2Params {
    t: 1,
    m_kib: 8,
    p: 1,
};

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

#[derive(Debug)]
struct Session {
    bearer: String,
    channel_binding: String,
}

async fn login(http: &reqwest::Client, base: &str, account: &Account) -> Session {
    let client = ClientHandshake::new().expect("client handshake");
    let a_pub = client.public().to_vec();

    let resp = http
        .post(format!("{base}/v1/auth/srp/start"))
        .json(&serde_json::json!({ "username": account.username, "a_pub": b64(&a_pub) }))
        .send()
        .await
        .expect("srp start");
    assert!(resp.status().is_success(), "srp start: {}", resp.status());
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
    assert!(resp.status().is_success(), "srp finish: {}", resp.status());
    let finish: serde_json::Value = resp.json().await.expect("finish json");
    let m2 = b64_decode(finish["m2"].as_str().expect("m2"));

    let session = client_login.verify_server(&m2).expect("verify server m2");
    let bearer = session.derive_bearer_token().expect("bearer");
    let channel_binding = session.derive_channel_binding().expect("channel");
    Session {
        bearer: hex(bearer.expose_secret()),
        channel_binding: hex(&channel_binding),
    }
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

/// Register Alice (bootstraps admin), log her in, register a device, upload a
/// vault header, and push `payloads` as events. Returns
/// `(session, admin_token, device_id_hex, header_bytes)`.
async fn seed_alice(
    http: &reqwest::Client,
    base: &str,
    vault_hex: &str,
    payloads: &[&[u8]],
    header: &[u8],
) -> (Session, String, String) {
    let alice = Account::new("alice", b"alice-password", b"alice-secret-key");
    let reg = register(http, base, &alice.register_body(None)).await;
    assert_eq!(reg["role"], "admin");
    let admin_token = reg["admin_token"]
        .as_str()
        .expect("admin token")
        .to_string();
    let sess = login(http, base, &alice).await;

    let device_id = hex(&[0xD1; 16]);
    let resp = http
        .post(format!("{base}/v1/vaults/{vault_hex}/devices"))
        .bearer_auth(&sess.bearer)
        .json(&serde_json::json!({
            "device_id": device_id,
            "verifying_key": hex(&[0xE2; 32]),
            "label": "alice-device",
        }))
        .send()
        .await
        .expect("register device");
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);

    let resp = http
        .put(format!("{base}/v1/vaults/{vault_hex}/header"))
        .bearer_auth(&sess.bearer)
        .json(&serde_json::json!({ "header": b64(header) }))
        .send()
        .await
        .expect("put header");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    for (i, payload) in payloads.iter().enumerate() {
        let mut eid = [0_u8; 16];
        eid[0] = u8::try_from(i + 1).unwrap();
        let resp = http
            .post(format!("{base}/v1/vaults/{vault_hex}/events/push"))
            .bearer_auth(&sess.bearer)
            .header("X-Tock-Channel-Binding", &sess.channel_binding)
            .json(&serde_json::json!({
                "events": [{
                    "event_id": hex(&eid),
                    "device_id": device_id,
                    "lamport": i64::try_from(i + 1).unwrap(),
                    "payload": b64(payload),
                }],
            }))
            .send()
            .await
            .expect("push");
        assert_eq!(resp.status(), reqwest::StatusCode::OK, "push {i}");
    }

    (sess, admin_token, device_id)
}

#[tokio::test]
async fn export_owner_happy_path_and_round_trip_import() {
    let source = TestServer::start();
    let http = reqwest::Client::new();
    let base = &source.base_url;
    let vault = hex(&[0xA0; 16]);
    let header = b"opaque-non-secret-vault-header";
    let ciphertexts: [&[u8]; 2] = [b"opaque-event-one", b"opaque-event-two"];

    let (sess, _admin, _device) = seed_alice(&http, base, &vault, &ciphertexts, header).await;

    // ── Owner exports: header + full ciphertext log, unchanged. ─────────
    let resp = http
        .get(format!("{base}/v1/vaults/{vault}/export"))
        .bearer_auth(&sess.bearer)
        .header("X-Tock-Channel-Binding", &sess.channel_binding)
        .send()
        .await
        .expect("export");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let archive: serde_json::Value = resp.json().await.expect("archive json");
    assert_eq!(archive["vault_id"], vault);
    assert_eq!(
        b64_decode(archive["header"].as_str().expect("header")),
        header
    );
    let events = archive["events"].as_array().expect("events").clone();
    assert_eq!(events.len(), 2);
    assert_eq!(
        b64_decode(events[0]["payload"].as_str().unwrap()),
        ciphertexts[0]
    );
    assert_eq!(
        b64_decode(events[1]["payload"].as_str().unwrap()),
        ciphertexts[1]
    );

    // ── Round-trip: import the archive into a FRESH vault on a SECOND
    //    server instance (models cross-server migration, #202). ──────────
    let dest = TestServer::start();
    let dbase = &dest.base_url;
    // Carol bootstraps admin on the fresh instance, then imports.
    let carol = Account::new("carol", b"carol-password", b"carol-secret-key");
    register(&http, dbase, &carol.register_body(None)).await;
    let csess = login(&http, dbase, &carol).await;
    let new_vault = hex(&[0xC7; 16]);

    let import_body = serde_json::json!({
        "vault_id": archive["vault_id"],
        "header": archive["header"],
        "events": events,
    });
    let resp = http
        .post(format!("{dbase}/v1/vaults/{new_vault}/import"))
        .bearer_auth(&csess.bearer)
        .header("X-Tock-Channel-Binding", &csess.channel_binding)
        .json(&import_body)
        .send()
        .await
        .expect("import");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let imported: serde_json::Value = resp.json().await.expect("import json");
    assert_eq!(imported["accepted"], 2);
    assert_eq!(imported["duplicates"], 0);

    // Pull from the destination vault: same ciphertext, in order.
    let resp = http
        .get(format!("{dbase}/v1/vaults/{new_vault}/events/pull"))
        .bearer_auth(&csess.bearer)
        .header("X-Tock-Channel-Binding", &csess.channel_binding)
        .send()
        .await
        .expect("pull dest");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let pulled: serde_json::Value = resp.json().await.expect("pull json");
    let dest_events = pulled["events"].as_array().expect("events");
    assert_eq!(dest_events.len(), 2);
    assert_eq!(
        b64_decode(dest_events[0]["payload"].as_str().unwrap()),
        ciphertexts[0]
    );
    assert_eq!(
        b64_decode(dest_events[1]["payload"].as_str().unwrap()),
        ciphertexts[1]
    );

    // Header round-tripped too.
    let resp = http
        .get(format!("{dbase}/v1/vaults/{new_vault}/header"))
        .bearer_auth(&csess.bearer)
        .send()
        .await
        .expect("get dest header");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let hdr: serde_json::Value = resp.json().await.expect("header json");
    assert_eq!(b64_decode(hdr["header"].as_str().unwrap()), header);

    // ── Idempotent: re-importing the same archive stores nothing new. ───
    let resp = http
        .post(format!("{dbase}/v1/vaults/{new_vault}/import"))
        .bearer_auth(&csess.bearer)
        .header("X-Tock-Channel-Binding", &csess.channel_binding)
        .json(&import_body)
        .send()
        .await
        .expect("re-import");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let reimport: serde_json::Value = resp.json().await.expect("reimport json");
    assert_eq!(reimport["accepted"], 0);
    assert_eq!(reimport["duplicates"], 2);
}

#[tokio::test]
async fn export_authorization_is_idor_safe() {
    let server = TestServer::start();
    let http = reqwest::Client::new();
    let base = &server.base_url;
    let vault = hex(&[0xA0; 16]);
    let header = b"opaque-header";
    let ciphertexts: [&[u8]; 1] = [b"opaque-event"];

    let (sess_a, admin_token, _device) =
        seed_alice(&http, base, &vault, &ciphertexts, header).await;

    // Anonymous export is rejected (401).
    let resp = http
        .get(format!("{base}/v1/vaults/{vault}/export"))
        .send()
        .await
        .expect("anon export");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Missing / wrong channel-binding tag is rejected (401).
    let resp = http
        .get(format!("{base}/v1/vaults/{vault}/export"))
        .bearer_auth(&sess_a.bearer)
        .send()
        .await
        .expect("export no binding");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
    let resp = http
        .get(format!("{base}/v1/vaults/{vault}/export"))
        .bearer_auth(&sess_a.bearer)
        .header("X-Tock-Channel-Binding", hex(&[0x00; 32]))
        .send()
        .await
        .expect("export wrong binding");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Bob (a plain user) registers and logs in.
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
    let bob_reg = register(&http, base, &bob.register_body(Some(invite_token))).await;
    let bob_account_id = bob_reg["account_id"].as_str().expect("bob id").to_string();
    let sess_b = login(&http, base, &bob).await;

    // Cross-account export is forbidden (403) — the IDOR the double-auth closes.
    let resp = http
        .get(format!("{base}/v1/vaults/{vault}/export"))
        .bearer_auth(&sess_b.bearer)
        .header("X-Tock-Channel-Binding", &sess_b.channel_binding)
        .send()
        .await
        .expect("bob exports alice");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    // Ownership-scoped, not admin-gated: Bob (a user) exports HIS OWN vault.
    let bob_vault = hex(&[0xB0; 16]);
    let resp = http
        .post(format!("{base}/v1/vaults/{bob_vault}/events/push"))
        .bearer_auth(&sess_b.bearer)
        .header("X-Tock-Channel-Binding", &sess_b.channel_binding)
        .json(&serde_json::json!({
            "events": [{
                "event_id": hex(&[0x42; 16]),
                "device_id": hex(&[0xD2; 16]),
                "lamport": 1,
                "payload": b64(b"bob-ciphertext"),
            }],
        }))
        .send()
        .await
        .expect("bob push own");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let resp = http
        .get(format!("{base}/v1/vaults/{bob_vault}/export"))
        .bearer_auth(&sess_b.bearer)
        .header("X-Tock-Channel-Binding", &sess_b.channel_binding)
        .send()
        .await
        .expect("bob exports own");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    // Disabled account is denied: admin disables Bob; his live session dies.
    let resp = http
        .post(format!("{base}/v1/admin/users/{bob_account_id}/disable"))
        .bearer_auth(&admin_token)
        .send()
        .await
        .expect("disable bob");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let resp = http
        .get(format!("{base}/v1/vaults/{bob_vault}/export"))
        .bearer_auth(&sess_b.bearer)
        .header("X-Tock-Channel-Binding", &sess_b.channel_binding)
        .send()
        .await
        .expect("disabled export");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn import_authorization_is_idor_safe() {
    let server = TestServer::start();
    let http = reqwest::Client::new();
    let base = &server.base_url;
    let vault = hex(&[0xA0; 16]);
    let header = b"opaque-header";
    let ciphertexts: [&[u8]; 1] = [b"opaque-event"];

    let (_sess_a, admin_token, _device) =
        seed_alice(&http, base, &vault, &ciphertexts, header).await;

    let archive = serde_json::json!({
        "vault_id": vault,
        "events": [{
            "event_id": hex(&[0x99; 16]),
            "device_id": hex(&[0xD3; 16]),
            "lamport": 1,
            "payload": b64(b"injected-ciphertext"),
        }],
    });

    // Anonymous import is rejected (401).
    let resp = http
        .post(format!("{base}/v1/vaults/{vault}/import"))
        .json(&archive)
        .send()
        .await
        .expect("anon import");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Bob registers + logs in.
    let resp = http
        .post(format!("{base}/v1/admin/users"))
        .bearer_auth(&admin_token)
        .json(&serde_json::json!({ "role": "user" }))
        .send()
        .await
        .expect("mint invite");
    let invite: serde_json::Value = resp.json().await.expect("invite json");
    let invite_token = invite["invite_token"].as_str().expect("invite token");
    let bob = Account::new("bob", b"bob-password", b"bob-secret-key");
    register(&http, base, &bob.register_body(Some(invite_token))).await;
    let sess_b = login(&http, base, &bob).await;

    // Missing channel-binding is rejected (401).
    let resp = http
        .post(format!("{base}/v1/vaults/{vault}/import"))
        .bearer_auth(&sess_b.bearer)
        .json(&archive)
        .send()
        .await
        .expect("import no binding");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Cross-account import into Alice's vault is forbidden (403): Bob cannot
    // overwrite/append to a vault owned by another account.
    let resp = http
        .post(format!("{base}/v1/vaults/{vault}/import"))
        .bearer_auth(&sess_b.bearer)
        .header("X-Tock-Channel-Binding", &sess_b.channel_binding)
        .json(&archive)
        .send()
        .await
        .expect("bob imports alice vault");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}
