//! Shared black-box harness for the `tock` CLI end-to-end tests.
//!
//! Provides an in-process [`TestServer`] (a real `tock-server` on an ephemeral
//! port) and a [`Device`] wrapper that drives the actual `tock` binary with an
//! isolated vault, `HOME`, and credential file — so several simulated devices,
//! and several servers, can coexist in one test.
//!
//! `tock-server` is consumed only as a dev-dependency; it never links into the
//! distributed Apache-2.0 CLI binary (ADR-006).

// Each integration-test binary uses a different subset of this harness, so
// per-binary dead-code warnings are expected and not meaningful.
#![allow(dead_code)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tock_server::ServerMode;

/// One account password shared by every device: in the account model a single
/// password (with the Secret Key) unlocks the vault and drives SRP, so a second
/// device must reuse the first device's password.
pub const ACCOUNT_PASSWORD: &str = "correct horse battery staple";

// ── In-process server harness ────────────────────────────────────────

/// A `tock-server` running on a background thread, bound to an ephemeral
/// port. Kept alive (temp dir + thread) for the lifetime of the value.
pub struct TestServer {
    pub base_url: String,
    pub db_path: PathBuf,
    _tmp: tempfile::TempDir,
}

impl TestServer {
    pub fn start() -> Self {
        let tmp = tempfile::tempdir().expect("server tmp dir");
        let data_dir = tmp.path().to_path_buf();
        let db_path = data_dir.join("tock-server.db");
        let (tx, rx) = std::sync::mpsc::channel();

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
            db_path,
            _tmp: tmp,
        }
    }

    /// Every opaque event payload (and onboarding blob) the server has
    /// stored, for the ciphertext-only assertion.
    pub fn stored_blobs(&self) -> Vec<Vec<u8>> {
        // Open read-only; retry briefly in case the server is mid-write.
        let conn = open_readonly_with_retry(&self.db_path);
        let mut blobs = Vec::new();
        for (table, col) in [("server_events", "payload"), ("onboarding_blobs", "blob")] {
            let sql = format!("SELECT {col} FROM {table}");
            let mut stmt = conn.prepare(&sql).expect("prepare blob query");
            let rows = stmt
                .query_map([], |row| row.get::<_, Vec<u8>>(0))
                .expect("query blobs");
            for row in rows {
                blobs.push(row.expect("read blob"));
            }
        }
        blobs
    }
}

fn open_readonly_with_retry(path: &Path) -> rusqlite::Connection {
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match rusqlite::Connection::open_with_flags(path, flags) {
            Ok(conn) => return conn,
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("cannot open server db read-only: {e}"),
        }
    }
}

// ── Driving the `tock` CLI ───────────────────────────────────────────

/// A single device: a vault path + password + an isolated HOME/config so the
/// CLI never picks up the developer's real hooks/contexts/config and each
/// device gets its own on-disk SRP-session credential file.
pub struct Device {
    pub vault: PathBuf,
    pub password: String,
    home: tempfile::TempDir,
    /// The account Secret Key (`A4-…` Emergency-Kit string). Captured from
    /// signup on device A, or adopted from A when a second device logs in.
    secret_key: std::sync::Mutex<Option<String>>,
}

impl Device {
    pub fn new(dir: &Path, name: &str) -> Self {
        Self {
            vault: dir.join(format!("{name}.tockvault")),
            password: ACCOUNT_PASSWORD.to_string(),
            home: tempfile::tempdir().expect("home tmp dir"),
            secret_key: std::sync::Mutex::new(None),
        }
    }

    /// The captured account Secret Key, if this device has been initialised
    /// (or has adopted the account owner's) yet.
    pub fn secret_key(&self) -> Option<String> {
        self.secret_key.lock().expect("secret-key lock").clone()
    }

    /// Adopt an account Secret Key (e.g. the owner's, when a second device
    /// logs in to the same account).
    pub fn set_secret_key(&self, key: String) {
        *self.secret_key.lock().expect("secret-key lock") = Some(key);
    }

    pub fn command(&self) -> Command {
        let mut cmd = Command::new(tock_bin());
        cmd.env("TOCK_VAULT", &self.vault)
            .env("TOCK_PASSWORD", &self.password)
            // Isolate per-device config/hook discovery.
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            // Route SRP session credentials to a per-config-dir file instead
            // of the single shared OS keyring slot, so the devices don't
            // clobber each other's session (and CI headless boxes have no
            // secret service).
            .env("TOCK_NO_KEYRING", "1")
            .env_remove("TOCK_SERVER");
        if let Some(secret_key) = self.secret_key() {
            cmd.env("TOCK_SECRET_KEY", secret_key);
        }
        cmd
    }

    /// Run `tock <args>` and return stdout, panicking with stderr on a
    /// non-zero exit.
    pub fn run(&self, args: &[&str]) -> String {
        let output = self.command().args(args).output().expect("spawn tock");
        assert!(
            output.status.success(),
            "tock {args:?} failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8(output.stdout).expect("utf8 stdout")
    }

    /// Run `tock <args>` expecting a **failure**, returning stderr for
    /// inspection. Panics if the command unexpectedly succeeds.
    pub fn run_expecting_failure(&self, args: &[&str]) -> String {
        let output = self.command().args(args).output().expect("spawn tock");
        assert!(
            !output.status.success(),
            "tock {args:?} unexpectedly succeeded\nstdout:\n{}",
            String::from_utf8_lossy(&output.stdout),
        );
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    /// The non-deleted tasks this vault currently sees, as JSON.
    pub fn tasks(&self) -> Vec<TaskRow> {
        let json = self.run(&["--format", "json", "list"]);
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("parse list json: {e}\nraw: {json}"))
    }

    pub fn sid_for(&self, title: &str) -> u32 {
        self.tasks()
            .into_iter()
            .find(|t| t.title == title)
            .unwrap_or_else(|| panic!("no task titled {title:?} in {}", self.vault.display()))
            .sid
    }

    /// The (title -> status) map, the device-agnostic convergence view
    /// (SIDs may differ across devices, titles are stable).
    pub fn title_status(&self) -> HashMap<String, String> {
        self.tasks()
            .into_iter()
            .map(|t| (t.title, t.status))
            .collect()
    }
}

/// Path to the freshly built `tock` binary, provided by Cargo.
pub const fn tock_bin() -> &'static str {
    env!("CARGO_BIN_EXE_tock")
}

#[derive(Debug, Deserialize)]
pub struct TaskRow {
    pub sid: u32,
    pub title: String,
    pub status: String,
}

// ── Account signup / login orchestration ─────────────────────────────

/// The one-time artifacts a signup surfaces for adding another device.
pub struct Signup {
    pub secret_key: String,
    pub setup_code: String,
}

/// Sign `device` up for a brand-new account against `server`. Returns the
/// `A4-…` Secret Key and the `TOCK1:` Setup Code printed in the Emergency
/// Kit, and adopts the Secret Key on the device so subsequent vault-opening
/// commands can unlock.
pub fn signup(server: &str, device: &Device, email: &str) -> Signup {
    let out = device.run(&["account", "signup", "--server", server, "--email", email]);
    let secret_key = find_token(&out, "A4-")
        .unwrap_or_else(|| panic!("no `A4-` Secret Key in signup output:\n{out}"));
    let setup_code = find_token(&out, "TOCK1:")
        .unwrap_or_else(|| panic!("no `TOCK1:` Setup Code in signup output:\n{out}"));
    device.set_secret_key(secret_key.clone());
    Signup {
        secret_key,
        setup_code,
    }
}

/// Log `device` in to an existing account using the owner's `TOCK1:` Setup
/// Code (which carries server + email + Secret Key). The device adopts the
/// account Secret Key so it can unlock the materialised vault afterwards.
pub fn login_with_setup_code(device: &Device, signup: &Signup) {
    device.set_secret_key(signup.secret_key.clone());
    device.run(&["account", "login", "--setup-code", &signup.setup_code]);
}

/// The first whitespace-delimited token in `output` starting with `prefix`.
/// Both the `A4-…` Secret Key and the `TOCK1:…` Setup Code are single,
/// space-free tokens, so this cleanly extracts them from the kit text.
pub fn find_token(output: &str, prefix: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|tok| tok.starts_with(prefix))
        .map(str::to_string)
}

// ── Assertion helpers ────────────────────────────────────────────────

/// Whether `haystack` contains the byte sequence `needle`.
pub fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Assert that none of `markers` appears in any of `blobs`.
pub fn assert_no_plaintext(blobs: &[Vec<u8>], markers: &[&str]) {
    for marker in markers {
        for blob in blobs {
            assert!(
                !contains_bytes(blob, marker.as_bytes()),
                "plaintext marker {marker:?} leaked into a server blob"
            );
        }
    }
}
