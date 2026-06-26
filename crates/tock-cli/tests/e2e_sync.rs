//! End-to-end multi-device sync acceptance test (issue #124).
//!
//! This is the cross-component test the per-crate unit suites never
//! covered: it spins up a real `tock-server` in-process on an ephemeral
//! port and drives **two actual `tock` CLI binaries** through the full
//! loop — add / modify / done, device pairing via the real interactive
//! `tock onboard` handshake, and `tock sync` — then asserts:
//!
//! 1. the two vaults converge on the same task set (CLI ⇄ server ⇄ CLI);
//! 2. the server store holds **only ciphertext** (no plaintext titles);
//! 3. concurrent same-field edits surface a conflict for review rather
//!    than silently clobbering (no last-write-wins) per ADR-003.
//!
//! The CLI is fully scriptable via `TOCK_VAULT` / `TOCK_PASSWORD`, so the
//! test treats it as a black box and never links the CLI's internals.
//! `tock-server` is consumed only as a dev-dependency (it never links
//! into the distributed Apache-2.0 CLI binary; see ADR-006).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tock_server::ServerMode;

/// Generous per-process / per-pairing timeout so a wedged child can never
/// hang CI indefinitely (the happy path completes in a couple seconds).
const STEP_TIMEOUT: Duration = Duration::from_secs(90);

/// Path to the freshly built `tock` binary, provided by Cargo.
const fn tock_bin() -> &'static str {
    env!("CARGO_BIN_EXE_tock")
}

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

/// A single device: a vault path + password + an isolated HOME so the
/// CLI never picks up the developer's real hooks/contexts/config.
struct Device {
    vault: PathBuf,
    password: String,
    home: tempfile::TempDir,
}

impl Device {
    fn new(dir: &Path, name: &str) -> Self {
        Self {
            vault: dir.join(format!("{name}.tockvault")),
            password: format!("pw-{name}"),
            home: tempfile::tempdir().expect("home tmp dir"),
        }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(tock_bin());
        cmd.env("TOCK_VAULT", &self.vault)
            .env("TOCK_PASSWORD", &self.password)
            // Isolate per-device config/hook discovery.
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env_remove("TOCK_SERVER");
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

#[derive(Debug, Deserialize)]
struct TaskRow {
    sid: u32,
    title: String,
    status: String,
}

// ── Interactive onboarding orchestration ─────────────────────────────

/// Pair `acceptor` to `inviter`'s vault by driving the real two-sided
/// `tock onboard invite` / `tock onboard accept` handshake, shuttling the
/// out-of-band values between the two processes exactly as a human would.
fn pair(server: &str, inviter: &Device, acceptor: &Device) {
    // 1. Start the inviter. It prints its --vault-id / --inviter-pubkey /
    //    --inviter-fingerprint, then blocks reading the acceptor's values
    //    from stdin.
    let mut invite = inviter
        .command()
        .args(["onboard", "invite", "--server", server])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn onboard invite");

    let invite_out = spawn_line_reader(invite.stdout.take().expect("invite stdout"));
    let inviter_vals = collect_values(
        &invite_out,
        &["--vault-id", "--inviter-pubkey", "--inviter-fingerprint"],
        STEP_TIMEOUT,
    );

    // 2. Start the acceptor with the inviter's values. It prints its own
    //    public key / fingerprint / device id, then polls for the blob.
    let mut accept = acceptor
        .command()
        .args([
            "onboard",
            "accept",
            "--server",
            server,
            "--vault-id",
            &inviter_vals["--vault-id"],
            "--inviter-pubkey",
            &inviter_vals["--inviter-pubkey"],
            "--inviter-fingerprint",
            &inviter_vals["--inviter-fingerprint"],
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn onboard accept");

    let accept_out = spawn_line_reader(accept.stdout.take().expect("accept stdout"));
    let acceptor_vals = collect_values(
        &accept_out,
        &[
            "Acceptor public key",
            "Acceptor fingerprint",
            "Acceptor device id",
        ],
        STEP_TIMEOUT,
    );

    // 3. Feed the acceptor's values into the inviter's stdin prompts (in
    //    the order the inviter reads them).
    {
        let mut stdin = invite.stdin.take().expect("invite stdin");
        writeln!(stdin, "{}", acceptor_vals["Acceptor public key"]).expect("write pubkey");
        writeln!(stdin, "{}", acceptor_vals["Acceptor fingerprint"]).expect("write fp");
        writeln!(stdin, "{}", acceptor_vals["Acceptor device id"]).expect("write dev id");
        // Dropping stdin closes the pipe.
    }

    // 4. Both sides should now complete: the inviter uploads the wrapped
    //    vault key, the acceptor's poll picks it up and builds its vault.
    let inv_status = wait_or_kill(&mut invite, STEP_TIMEOUT, "onboard invite");
    let acc_status = wait_or_kill(&mut accept, STEP_TIMEOUT, "onboard accept");
    assert!(inv_status.success(), "onboard invite exited {inv_status:?}");
    assert!(acc_status.success(), "onboard accept exited {acc_status:?}");
}

/// Read a child's stdout line-by-line into a channel on its own thread,
/// so the main thread never blocks on a half-written prompt line.
fn spawn_line_reader(stdout: std::process::ChildStdout) -> Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

/// Collect, for each `key`, the last whitespace token of the first line
/// containing that key. Bounded by `timeout`.
fn collect_values(
    rx: &Receiver<String>,
    keys: &[&str],
    timeout: Duration,
) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let deadline = Instant::now() + timeout;
    while out.len() < keys.len() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or_default();
        match rx.recv_timeout(remaining) {
            Ok(line) => {
                for key in keys {
                    if !out.contains_key(*key)
                        && line.contains(key)
                        && let Some(value) = line.split_whitespace().last()
                    {
                        out.insert((*key).to_string(), value.to_string());
                    }
                }
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                panic!("timed out collecting {keys:?}; got {out:?}");
            }
        }
    }
    out
}

/// Poll a child to completion, killing it if it overruns `timeout`.
fn wait_or_kill(child: &mut Child, timeout: Duration, name: &str) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => return status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{name} timed out after {timeout:?}");
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

/// Full happy path: two CLI vaults sync through one server and converge,
/// and the server only ever stores ciphertext.
#[test]
fn two_device_sync_converges_and_server_stores_only_ciphertext() {
    let server = TestServer::start();
    let dir = tempfile::tempdir().expect("work dir");
    let a = Device::new(dir.path(), "a");
    let b = Device::new(dir.path(), "b");

    // Distinctive, space-free plaintext markers. None of these substrings
    // may ever appear in the server's stored blobs.
    let markers = ["AlphaTaskZZ", "CanaryZZ", "BetaTaskZZ", "BetaEditedZZ"];

    // Device A: create the vault, add two tasks, push to the server.
    a.run(&["add", "AlphaTaskZZ"]);
    a.run(&["add", "CanaryZZ"]);
    a.run(&["sync", "--server", &server.base_url]);

    // Pair device B via the real interactive onboarding handshake; B pulls
    // the existing history during onboarding.
    pair(&server.base_url, &a, &b);

    let pending = b
        .tasks()
        .into_iter()
        .find(|t| t.title == "CanaryZZ")
        .expect("B pulled Canary during onboarding")
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
    a.run(&["add", "GammaZZ"]);
    a.run(&["sync", "--server", &server.base_url]);
    pair(&server.base_url, &a, &b);
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
