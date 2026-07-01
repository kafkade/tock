//! HTTP acceptance test for the public `GET /v1/server/info` endpoint
//! (issue #132 first-run gating for the web console).
//!
//! Asserts:
//! 1. on a fresh instance the endpoint is reachable **without auth** and
//!    reports `setup_required = true` with the default registration policy;
//! 2. after the first account registers (bootstrapped as admin), the same
//!    endpoint reports `setup_required = false`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::net::SocketAddr;
use std::path::PathBuf;

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

/// Minimal base64 (std alphabet, padded) for SRP material in the register body.
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

fn register_body(username: &str) -> serde_json::Value {
    serde_json::json!({
        "username": username,
        "srp_salt": b64(&[0xAA; 16]),
        "srp_verifier": b64(&[0xBB; 64]),
        "srp_group": "RFC5054-4096-SHA256",
        "kdf_params": { "alg": "argon2id", "t": 3, "m": 65536, "p": 1 },
    })
}

#[tokio::test]
async fn server_info_reports_setup_state() {
    let server = TestServer::start();
    let http = reqwest::Client::new();
    let base = &server.base_url;

    // 1. Fresh instance: reachable without auth, setup required.
    let resp = http
        .get(format!("{base}/v1/server/info"))
        .send()
        .await
        .expect("get server info");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let info: serde_json::Value = resp.json().await.expect("info json");
    assert_eq!(info["setup_required"], true);
    assert_eq!(info["mode"], "self-hosted");
    assert!(
        info["registration_policy"].is_string(),
        "policy present: {info}"
    );
    assert!(info["version"].is_string(), "version present: {info}");

    // 2. After the first (admin) account registers, setup is no longer required.
    let resp = http
        .post(format!("{base}/v1/accounts/register"))
        .json(&register_body("admin"))
        .send()
        .await
        .expect("register admin");
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);

    let resp = http
        .get(format!("{base}/v1/server/info"))
        .send()
        .await
        .expect("get server info again");
    let info: serde_json::Value = resp.json().await.expect("info json 2");
    assert_eq!(info["setup_required"], false);
}
