//! End-to-end signup → login round-trip for the shared `tock-account` layer.
//!
//! Boots a real `tock-server`, creates a local vault with `tock-storage`,
//! derives signup material, registers over HTTP, then runs the SRP login
//! state machine and proves the resulting bearer token + channel binding
//! authorize a vault-header upload. No password ever crosses the wire.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::net::SocketAddr;
use std::path::PathBuf;

use tock_account::login::LoginStart;
use tock_account::signup::SignupMaterial;
use tock_account::{FinishResponse, RegisterResponse, StartResponse};

struct TestServer {
    base_url: String,
    _tmp: tempfile::TempDir,
}

impl TestServer {
    fn start() -> Self {
        let tmp = tempfile::tempdir().expect("server tmp");
        let data_dir: PathBuf = tmp.path().to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel::<SocketAddr>();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("rt");
            rt.block_on(async move {
                let state =
                    tock_server::open_app_state(&data_dir, tock_server::ServerMode::SelfHosted)
                        .expect("state");
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind");
                let addr = listener.local_addr().expect("addr");
                tx.send(addr).expect("send");
                tock_server::serve(listener, state).await.expect("serve");
            });
        });
        let addr = rx.recv().expect("bind");
        Self {
            base_url: format!("http://{addr}"),
            _tmp: tmp,
        }
    }
}

#[tokio::test]
async fn signup_then_login_round_trip() {
    let server = TestServer::start();
    let http = reqwest::Client::new();
    let username = "alice@example.com";
    let password = "correct horse battery staple";

    // Local vault + signup material.
    let dir = tempfile::tempdir().expect("vault dir");
    let (vault, secret_key) =
        tock_storage::vault::init(&dir.path().join("a.tockvault"), password.as_bytes())
            .expect("init");
    let header = vault.header();
    let material =
        SignupMaterial::derive(username, password, &secret_key, header, &server.base_url)
            .expect("derive");

    // Register.
    let resp = http
        .post(format!("{}/v1/accounts/register", server.base_url))
        .json(&material.register_request)
        .send()
        .await
        .expect("register");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::CREATED,
        "register status"
    );
    let reg: RegisterResponse = resp.json().await.expect("reg json");
    assert_eq!(reg.status, "active");

    // Login: start.
    let (start, start_req) = LoginStart::new(username).expect("login start");
    let sresp: StartResponse = http
        .post(format!("{}/v1/auth/srp/start", server.base_url))
        .json(&start_req)
        .send()
        .await
        .expect("start")
        .json()
        .await
        .expect("start json");
    assert!(!sresp.kdf_params.is_null(), "kdf params echoed back");

    // Login: finish.
    let (pending, finish_req) = start.finish(&sresp, password, &secret_key).expect("finish");
    let fresp: FinishResponse = http
        .post(format!("{}/v1/auth/srp/finish", server.base_url))
        .json(&finish_req)
        .send()
        .await
        .expect("finish")
        .json()
        .await
        .expect("finish json");
    let session = pending.verify(&fresp).expect("verify m2");
    assert_eq!(session.bearer_token.len(), 64);
    assert_eq!(session.channel_binding.len(), 64);

    // The session token authorizes a header upload (Phase S).
    let vid = tock_account::codec::hex_encode(vault.header().vault_id.as_bytes());
    let body = serde_json::json!({
        "header": tock_account::codec::base64_encode(&vault.header().to_bytes()),
    });
    let put = http
        .put(format!("{}/v1/vaults/{vid}/header", server.base_url))
        .header("authorization", format!("Bearer {}", session.bearer_token))
        .header("x-tock-channel-binding", &session.channel_binding)
        .json(&body)
        .send()
        .await
        .expect("put header");
    assert_eq!(
        put.status(),
        reqwest::StatusCode::OK,
        "authed header upload"
    );
}

#[tokio::test]
async fn wrong_password_rejected() {
    let server = TestServer::start();
    let http = reqwest::Client::new();
    let username = "bob@example.com";
    let dir = tempfile::tempdir().expect("vault dir");
    let (vault, secret_key) =
        tock_storage::vault::init(&dir.path().join("b.tockvault"), b"right-pass").expect("init");
    let material = SignupMaterial::derive(
        username,
        "right-pass",
        &secret_key,
        vault.header(),
        &server.base_url,
    )
    .expect("derive");
    let _ = http
        .post(format!("{}/v1/accounts/register", server.base_url))
        .json(&material.register_request)
        .send()
        .await
        .expect("register");

    let (start, start_req) = LoginStart::new(username).expect("start");
    let sresp: StartResponse = http
        .post(format!("{}/v1/auth/srp/start", server.base_url))
        .json(&start_req)
        .send()
        .await
        .expect("start")
        .json()
        .await
        .expect("json");
    let (_pending, finish_req) = start
        .finish(&sresp, "wrong-pass", &secret_key)
        .expect("finish");
    let fr = http
        .post(format!("{}/v1/auth/srp/finish", server.base_url))
        .json(&finish_req)
        .send()
        .await
        .expect("finish");
    assert_eq!(fr.status(), reqwest::StatusCode::UNAUTHORIZED);
}
