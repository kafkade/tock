//! End-to-end multi-device **account-authenticated** sync acceptance test
//! (issue #170).
//!
//! This is the flagship cross-component gate the per-crate unit suites never
//! covered: it spins up a real `tock-server` in-process on an ephemeral port
//! and drives **two actual `tock` CLI binaries** through the full account
//! loop — `tock account signup` (device A) → `tock account login` (device B,
//! via the Setup Code from A's Emergency Kit) → `tock add / modify / done`
//! → `tock sync` — then asserts:
//!
//! 1. the two vaults converge on the same task set (CLI ⇄ server ⇄ CLI);
//! 2. the server store holds **only ciphertext** (no plaintext titles);
//! 3. concurrent same-field edits surface a conflict for review rather
//!    than silently clobbering (no last-write-wins) per ADR-003;
//! 4. an **unauthenticated** client request is rejected with `401`.
//!
//! Background: the CLI HTTP transport authenticates every sync/onboarding
//! route with an SRP session (`Authorization: Bearer` +
//! `X-Tock-Channel-Binding`, see `http_transport.rs`), and `tock account
//! login/signup` shipped in #129 — so the authenticated **client** round-trip
//! can finally be proven here, not just server-side
//! (`tock-server/tests/srp_sync.rs`).
//!
//! The CLI is fully scriptable via `TOCK_VAULT` / `TOCK_PASSWORD` /
//! `TOCK_SECRET_KEY`, and `TOCK_NO_KEYRING=1` routes SRP session credentials
//! to a per-`XDG_CONFIG_HOME` file so the two devices stay isolated. The test
//! treats the CLI as a black box and never links its internals.
//! `tock-server` is consumed only as a dev-dependency (it never links into
//! the distributed Apache-2.0 CLI binary; see ADR-006).

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
const ACCOUNT_PASSWORD: &str = "correct horse battery staple";

// ── In-process server harness ────────────────────────────────────────

/// A `tock-server` running on a background thread, bound to an ephemeral
/// port. Kept alive (temp dir + thread) for the lifetime of the value.
struct TestServer {
    base_url: String,
    db_path: PathBuf,
    _tmp: tempfile::TempDir,
}

impl TestServer {
    fn start() -> Self {
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
    fn stored_blobs(&self) -> Vec<Vec<u8>> {
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
struct Device {
    vault: PathBuf,
    password: String,
    home: tempfile::TempDir,
    /// The account Secret Key (`A4-…` Emergency-Kit string). Captured from
    /// signup on device A, or adopted from A when a second device logs in.
    secret_key: std::sync::Mutex<Option<String>>,
}

impl Device {
    fn new(dir: &Path, name: &str) -> Self {
        Self {
            vault: dir.join(format!("{name}.tockvault")),
            password: ACCOUNT_PASSWORD.to_string(),
            home: tempfile::tempdir().expect("home tmp dir"),
            secret_key: std::sync::Mutex::new(None),
        }
    }

    /// The captured account Secret Key, if this device has been initialised
    /// (or has adopted the account owner's) yet.
    fn secret_key(&self) -> Option<String> {
        self.secret_key.lock().expect("secret-key lock").clone()
    }

    /// Adopt an account Secret Key (e.g. the owner's, when a second device
    /// logs in to the same account).
    fn set_secret_key(&self, key: String) {
        *self.secret_key.lock().expect("secret-key lock") = Some(key);
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(tock_bin());
        cmd.env("TOCK_VAULT", &self.vault)
            .env("TOCK_PASSWORD", &self.password)
            // Isolate per-device config/hook discovery.
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            // Route SRP session credentials to a per-config-dir file instead
            // of the single shared OS keyring slot, so the two devices don't
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
    fn run(&self, args: &[&str]) -> String {
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
    fn run_expecting_failure(&self, args: &[&str]) -> String {
        let output = self.command().args(args).output().expect("spawn tock");
        assert!(
            !output.status.success(),
            "tock {args:?} unexpectedly succeeded\nstdout:\n{}",
            String::from_utf8_lossy(&output.stdout),
        );
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    /// The non-deleted tasks this vault currently sees, as JSON.
    fn tasks(&self) -> Vec<TaskRow> {
        let json = self.run(&["--format", "json", "list"]);
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("parse list json: {e}\nraw: {json}"))
    }

    fn sid_for(&self, title: &str) -> u32 {
        self.tasks()
            .into_iter()
            .find(|t| t.title == title)
            .unwrap_or_else(|| panic!("no task titled {title:?} in {}", self.vault.display()))
            .sid
    }

    /// The (title -> status) map, the device-agnostic convergence view
    /// (SIDs may differ across devices, titles are stable).
    fn title_status(&self) -> HashMap<String, String> {
        self.tasks()
            .into_iter()
            .map(|t| (t.title, t.status))
            .collect()
    }
}

/// Path to the freshly built `tock` binary, provided by Cargo.
const fn tock_bin() -> &'static str {
    env!("CARGO_BIN_EXE_tock")
}

#[derive(Debug, Deserialize)]
struct TaskRow {
    sid: u32,
    title: String,
    status: String,
}

// ── Account signup / login orchestration ─────────────────────────────

/// Sign `device` up for a brand-new account against `server`. Returns the
/// `A4-…` Secret Key and the `TOCK1:` Setup Code printed in the Emergency
/// Kit, and adopts the Secret Key on the device so subsequent vault-opening
/// commands can unlock.
fn signup(server: &str, device: &Device, email: &str) -> Signup {
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

/// The one-time artifacts a signup surfaces for adding another device.
struct Signup {
    secret_key: String,
    setup_code: String,
}

/// Log `device` in to an existing account using the owner's `TOCK1:` Setup
/// Code (which carries server + email + Secret Key). The device adopts the
/// account Secret Key so it can unlock the materialised vault afterwards.
fn login_with_setup_code(device: &Device, signup: &Signup) {
    device.set_secret_key(signup.secret_key.clone());
    device.run(&["account", "login", "--setup-code", &signup.setup_code]);
}

/// The first whitespace-delimited token in `output` starting with `prefix`.
/// Both the `A4-…` Secret Key and the `TOCK1:…` Setup Code are single,
/// space-free tokens, so this cleanly extracts them from the kit text.
fn find_token(output: &str, prefix: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|tok| tok.starts_with(prefix))
        .map(str::to_string)
}

// ── Tests ────────────────────────────────────────────────────────────

/// Full happy path: device A signs up, device B logs in with A's Setup Code,
/// their two CLI vaults sync through one server and converge — and the server
/// only ever stores ciphertext.
#[test]
fn two_device_sync_converges_and_server_stores_only_ciphertext() {
    let server = TestServer::start();
    let dir = tempfile::tempdir().expect("work dir");
    let a = Device::new(dir.path(), "a");
    let b = Device::new(dir.path(), "b");

    // Distinctive, space-free plaintext markers. None of these substrings
    // may ever appear in the server's stored blobs.
    let markers = ["AlphaTaskZZ", "CanaryZZ", "BetaTaskZZ", "BetaEditedZZ"];

    // Device A: create the account + vault, add two tasks, push to the server.
    let signup = signup(&server.base_url, &a, "alice@example.com");
    a.run(&["add", "AlphaTaskZZ"]);
    a.run(&["add", "CanaryZZ"]);
    a.run(&["sync", "--server", &server.base_url]);

    // Device B: log in with A's Setup Code (fetches the vault header and
    // materialises the local vault), then sync to pull the existing history.
    login_with_setup_code(&b, &signup);
    b.run(&["sync", "--server", &server.base_url]);

    let pending = b
        .tasks()
        .into_iter()
        .find(|t| t.title == "CanaryZZ")
        .expect("B pulled Canary on first sync")
        .status;
    assert_eq!(b.title_status().len(), 2, "B should have A's two tasks");

    // Device B: add a task and push it.
    b.run(&["add", "BetaTaskZZ"]);
    b.run(&["sync"]);
    // Device A: pull B's new task.
    a.run(&["sync"]);

    // Exercise modify + done on BOTH devices, on different tasks.
    let alpha_a = a.sid_for("AlphaTaskZZ");
    a.run(&["done", &alpha_a.to_string()]);
    a.run(&["sync"]);

    let beta_b = b.sid_for("BetaTaskZZ");
    b.run(&["modify", &beta_b.to_string(), "title:BetaEditedZZ"]);
    b.run(&["sync"]);

    // Settle: one more round each way.
    a.run(&["sync"]);
    b.run(&["sync"]);

    // 1. Convergence: identical (title -> status) view on both devices.
    let a_view = a.title_status();
    let b_view = b.title_status();
    assert_eq!(a_view, b_view, "devices did not converge");

    // The modify propagated (Beta renamed) ...
    assert!(
        a_view.contains_key("BetaEditedZZ"),
        "modify did not propagate"
    );
    assert!(
        !a_view.contains_key("BetaTaskZZ"),
        "stale Beta title remains"
    );
    // ... and the done propagated (Alpha no longer in the pending state).
    let alpha_status = a_view.get("AlphaTaskZZ").expect("Alpha present");
    assert_ne!(
        *alpha_status, pending,
        "done did not propagate; Alpha still {alpha_status:?}"
    );

    // 2. The server stored only ciphertext: no plaintext marker appears in
    //    any event payload or onboarding blob.
    let blobs = server.stored_blobs();
    assert!(!blobs.is_empty(), "server stored no events");
    for marker in markers {
        for blob in &blobs {
            assert!(
                !contains_bytes(blob, marker.as_bytes()),
                "plaintext marker {marker:?} leaked into a server blob"
            );
        }
    }
}

/// Concurrent edits to the same field on two devices must surface a
/// conflict for review — no silent last-write-wins (ADR-003).
#[test]
fn concurrent_same_field_edits_surface_a_conflict() {
    let server = TestServer::start();
    let dir = tempfile::tempdir().expect("work dir");
    let a = Device::new(dir.path(), "a");
    let b = Device::new(dir.path(), "b");

    // Shared starting point: one task known to both devices.
    let signup = signup(&server.base_url, &a, "alice@example.com");
    a.run(&["add", "GammaZZ"]);
    a.run(&["sync", "--server", &server.base_url]);

    login_with_setup_code(&b, &signup);
    b.run(&["sync", "--server", &server.base_url]);
    assert!(b.tasks().iter().any(|t| t.title == "GammaZZ"));

    // Both devices edit the SAME field (title) while neither has seen the
    // other's change.
    let gamma_a = a.sid_for("GammaZZ");
    let gamma_b = b.sid_for("GammaZZ");
    a.run(&["modify", &gamma_a.to_string(), "title:GammaFromAZZ"]);
    b.run(&["modify", &gamma_b.to_string(), "title:GammaFromBZZ"]);

    // A pushes first; B then pushes its own and pulls A's conflicting edit.
    a.run(&["sync"]);
    let b_sync = b.run(&["sync"]);

    // The sync output flags the conflict, and `tock sync conflicts` lists
    // it for review rather than silently clobbering.
    assert!(
        b_sync.contains("conflicts 1") || b_sync.contains("Review conflicts"),
        "expected B's sync to report a conflict, got:\n{b_sync}"
    );
    let conflicts = b.run(&["sync", "conflicts"]);
    assert!(
        conflicts.contains("Unresolved conflicts"),
        "expected an unresolved conflict on B, got:\n{conflicts}"
    );
    assert!(
        !conflicts.contains("No unresolved conflicts"),
        "B silently clobbered the concurrent edit"
    );

    // The conflict is resolvable (what `tock sync resolve <id>` drives).
    let id = parse_first_conflict_id(&conflicts);
    let resolved = b.run(&["sync", "resolve", &id]);
    assert!(resolved.contains("Resolved"), "resolve failed: {resolved}");
    let after = b.run(&["sync", "conflicts"]);
    assert!(
        after.contains("No unresolved conflicts"),
        "conflict still listed after resolve:\n{after}"
    );
}

/// The client path must not reach a self-hosted server unauthenticated: a
/// `tock sync` from a device that never signed in / logged in is rejected
/// with a `401`, surfaced as a CLI error (no partial, unauthenticated write).
#[test]
fn unauthenticated_sync_is_rejected_with_401() {
    let server = TestServer::start();
    let dir = tempfile::tempdir().expect("work dir");
    let solo = Device::new(dir.path(), "solo");

    // Create a local vault WITHOUT any account signup/login, so no SRP
    // session credentials are ever stored. `tock add` auto-initialises the
    // vault and prints the Emergency Kit; capture the Secret Key so the
    // follow-up `sync` can unlock the vault before it hits the network.
    let init_out = solo.run(&["add", "LonelyTaskZZ"]);
    let secret_key = find_token(&init_out, "A4-")
        .unwrap_or_else(|| panic!("no `A4-` Secret Key in init output:\n{init_out}"));
    solo.set_secret_key(secret_key);

    let stderr = solo.run_expecting_failure(&["sync", "--server", &server.base_url]);
    assert!(
        stderr.contains("401"),
        "expected an unauthenticated 401 rejection, got stderr:\n{stderr}"
    );

    // The server must not have persisted any event from the rejected client.
    assert!(
        server.stored_blobs().is_empty(),
        "server stored events from an unauthenticated client"
    );
}

/// Extract the first conflict id (the `[uuid]` token) from `tock sync
/// conflicts` output.
fn parse_first_conflict_id(listing: &str) -> String {
    for line in listing.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix('[')
            && let Some(end) = rest.find(']')
        {
            return rest[..end].to_string();
        }
    }
    panic!("no conflict id in:\n{listing}");
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
