//! End-to-end acceptance test for `tock account adopt` / `disconnect`
//! (issue #197, ADR-016).
//!
//! Adoption is the local-first → opt-in-server upgrade: it registers an
//! **existing** local vault with a chosen server while preserving the client
//! crypto identity — the `account_id` **A** and `vault_id` **V** are unchanged,
//! the server mints its own principal **B**, and the already-wrapped header is
//! uploaded verbatim (no VK rotation, no re-encryption). This suite spins up a
//! real in-process `tock-server` on an ephemeral port and drives the actual
//! `tock` CLI binary as a black box, asserting the seven acceptance criteria:
//!
//! * **AC #1** — `adopt` derives from the existing header: A and V are unchanged
//!   after adoption (read straight from the local `vault_meta`).
//! * **AC #3** — a vault bound to one server refuses a second `adopt` unless
//!   `--migrate` is passed (authoritative-server invariant, §4).
//! * **AC #4** — the server stores exactly the wrapped header the client holds
//!   (VK stays wrapped) and no plaintext / Secret Key leaks.
//! * **AC #5** — `tock account status` reflects the binding and `tock sync`
//!   round-trips.
//! * **AC #6** — a second device that logs in then re-adopts continues only when
//!   A **and** V match.
//! * **AC #7** — `disconnect` revokes B, clears credentials, and preserves the
//!   local vault; re-adopt works afterward.
//!
//! The CLI is scripted via `TOCK_VAULT` / `TOCK_PASSWORD` / `TOCK_SECRET_KEY`
//! with `TOCK_NO_KEYRING=1` routing SRP credentials to a per-config-dir file so
//! devices stay isolated. `tock-server` is a dev-dependency only (it never
//! links into the distributed Apache-2.0 CLI binary; see ADR-006).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use tock_core::vault::VaultHeader;
use tock_server::ServerMode;

/// One account password shared by every device (with the Secret Key it unlocks
/// the vault and drives SRP).
const ACCOUNT_PASSWORD: &str = "correct horse battery staple";

// ── In-process server harness ────────────────────────────────────────

/// A `tock-server` on a background thread bound to an ephemeral port.
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

    fn conn(&self) -> rusqlite::Connection {
        open_readonly_with_retry(&self.db_path)
    }

    /// Every opaque event payload the server has stored.
    fn stored_blobs(&self) -> Vec<Vec<u8>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT payload FROM server_events")
            .expect("prepare");
        let rows = stmt
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .expect("query");
        rows.map(|r| r.expect("row")).collect()
    }

    /// Whether an account with `username` currently exists.
    fn account_exists(&self, username: &str) -> bool {
        let conn = self.conn();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM accounts WHERE username = ?1",
                [username],
                |r| r.get(0),
            )
            .expect("count accounts");
        count > 0
    }

    /// The wrapped header the server stored for `vault_id` (16-byte id), if any.
    fn stored_header(&self, vault_id: &[u8]) -> Option<Vec<u8>> {
        let conn = self.conn();
        conn.query_row(
            "SELECT header FROM vault_headers WHERE vault_id = ?1",
            [vault_id],
            |r| r.get::<_, Vec<u8>>(0),
        )
        .ok()
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

/// A single device: a vault path + password + isolated HOME/config.
struct Device {
    vault: PathBuf,
    password: String,
    home: tempfile::TempDir,
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

    fn secret_key(&self) -> Option<String> {
        self.secret_key.lock().expect("secret-key lock").clone()
    }

    fn set_secret_key(&self, key: String) {
        *self.secret_key.lock().expect("secret-key lock") = Some(key);
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(tock_bin());
        cmd.env("TOCK_VAULT", &self.vault)
            .env("TOCK_PASSWORD", &self.password)
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("TOCK_NO_KEYRING", "1")
            .env_remove("TOCK_SERVER");
        if let Some(secret_key) = self.secret_key() {
            cmd.env("TOCK_SECRET_KEY", secret_key);
        }
        cmd
    }

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

    fn run_expecting_failure(&self, args: &[&str]) -> String {
        let output = self.command().args(args).output().expect("spawn tock");
        assert!(
            !output.status.success(),
            "tock {args:?} unexpectedly succeeded\nstdout:\n{}",
            String::from_utf8_lossy(&output.stdout),
        );
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    /// Create a fresh serverless vault by adding a task; returns the printed
    /// `A4-…` Secret Key (which embeds the crypto account id **A**), and adopts
    /// it on the device so later vault-opening commands can unlock.
    fn init_vault(&self, title: &str) -> String {
        let out = self.run(&["add", title]);
        let secret_key = find_token(&out, "A4-")
            .unwrap_or_else(|| panic!("no `A4-` Secret Key in init output:\n{out}"));
        self.set_secret_key(secret_key.clone());
        secret_key
    }

    fn titles(&self) -> Vec<String> {
        let json = self.run(&["--format", "json", "list"]);
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&json).unwrap_or_else(|e| panic!("parse list: {e}\n{json}"));
        rows.into_iter()
            .filter_map(|r| r.get("title").and_then(|t| t.as_str()).map(str::to_string))
            .collect()
    }

    /// Read a 16-byte `vault_meta` value (e.g. `vault_id`, `account_id`).
    fn meta_id(&self, key: &str) -> Vec<u8> {
        let conn = open_readonly_with_retry(&self.vault);
        conn.query_row("SELECT value FROM vault_meta WHERE key = ?1", [key], |r| {
            r.get::<_, Vec<u8>>(0)
        })
        .unwrap_or_else(|e| panic!("read vault_meta[{key}]: {e}"))
    }

    /// Reconstruct the local wrapped header bytes from `vault_meta` — exactly
    /// what `adopt` uploads to the server.
    fn local_header_bytes(&self) -> Vec<u8> {
        let conn = open_readonly_with_retry(&self.vault);
        let mut stmt = conn
            .prepare("SELECT key, value FROM vault_meta")
            .expect("prepare meta");
        let mut map: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .expect("query meta");
        for row in rows {
            let (k, v) = row.expect("meta row");
            map.insert(k, v);
        }
        VaultHeader::from_meta(&map)
            .expect("parse local header")
            .to_bytes()
    }
}

const fn tock_bin() -> &'static str {
    env!("CARGO_BIN_EXE_tock")
}

fn find_token(output: &str, prefix: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|tok| tok.starts_with(prefix))
        .map(str::to_string)
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

// ── Tests ────────────────────────────────────────────────────────────

/// AC #1, #4, #5: adopting an existing local vault preserves A and V, uploads
/// exactly the local wrapped header (no plaintext leak), reflects the binding in
/// `status`, and lets `tock sync` round-trip.
#[test]
fn adopt_preserves_ids_uploads_wrapped_header_and_syncs() {
    let server = TestServer::start();
    let dir = tempfile::tempdir().expect("work dir");
    let a = Device::new(dir.path(), "a");

    let secret_key = a.init_vault("AdoptMeZZ");

    // Snapshot A and V before adoption.
    let vault_id_before = a.meta_id("vault_id");
    let account_id_before = a.meta_id("account_id");
    let local_header = a.local_header_bytes();

    let adopt_out = a.run(&[
        "account",
        "adopt",
        "--server",
        &server.base_url,
        "--email",
        "alice@example.com",
    ]);
    assert!(
        adopt_out.contains("Adopted"),
        "adopt should confirm success:\n{adopt_out}"
    );

    // AC #1: A and V are unchanged after adoption (no rotation / re-init).
    assert_eq!(
        a.meta_id("vault_id"),
        vault_id_before,
        "vault_id (V) changed during adopt"
    );
    assert_eq!(
        a.meta_id("account_id"),
        account_id_before,
        "account_id (A) changed during adopt"
    );

    // AC #4: the server stored EXACTLY the client's wrapped header (VK stays
    // wrapped) and nothing leaked in plaintext.
    let stored = server
        .stored_header(&vault_id_before)
        .expect("server stored the vault header at registration");
    assert_eq!(
        stored, local_header,
        "server header != local wrapped header"
    );
    assert!(
        !contains_bytes(&stored, b"AdoptMeZZ"),
        "plaintext task title leaked into the stored header"
    );
    assert!(
        !contains_bytes(&stored, secret_key.as_bytes()),
        "Secret Key leaked into the stored header"
    );

    // AC #5: status reflects the binding, and sync round-trips.
    let status = a.run(&["account", "status"]);
    assert!(
        status.contains("alice@example.com") && status.contains(&server.base_url),
        "status should reflect the new binding:\n{status}"
    );
    let sync_out = a.run(&["sync"]);
    assert!(
        sync_out.contains(&format!("Synced with {}", server.base_url)),
        "sync should round-trip after adopt:\n{sync_out}"
    );

    // The pushed events are ciphertext-only (no plaintext marker).
    for blob in server.stored_blobs() {
        assert!(
            !contains_bytes(&blob, b"AdoptMeZZ"),
            "plaintext marker leaked into a server event"
        );
    }
}

/// AC #3: a vault bound to one server refuses a second `adopt` unless
/// `--migrate` is passed (authoritative-server invariant).
#[test]
fn adopt_refuses_second_server_without_migrate() {
    let s1 = TestServer::start();
    let s2 = TestServer::start();
    let dir = tempfile::tempdir().expect("work dir");
    let a = Device::new(dir.path(), "a");

    a.init_vault("BoundZZ");
    a.run(&[
        "account",
        "adopt",
        "--server",
        &s1.base_url,
        "--email",
        "alice@example.com",
    ]);

    // A second adopt onto a DIFFERENT server is refused and names --migrate.
    let stderr = a.run_expecting_failure(&[
        "account",
        "adopt",
        "--server",
        &s2.base_url,
        "--email",
        "alice@example.com",
    ]);
    assert!(
        stderr.contains("--migrate"),
        "refusal should mention --migrate, got:\n{stderr}"
    );
    // The original binding is intact: s1 still holds alice; s2 does not.
    assert!(s1.account_exists("alice@example.com"));
    assert!(!s2.account_exists("alice@example.com"));

    // With --migrate it proceeds and re-binds to the new server.
    let out = a.run(&[
        "account",
        "adopt",
        "--server",
        &s2.base_url,
        "--email",
        "alice@example.com",
        "--migrate",
    ]);
    assert!(
        out.contains("Adopted"),
        "migrate adopt should succeed:\n{out}"
    );
    assert!(s2.account_exists("alice@example.com"));
    // Reconciliation (AC #5, ADR-016 §4/Q3): `--migrate` disconnected the old
    // binding — B was revoked on s1 (its account row is gone) and the local
    // binding moved entirely to s2.
    assert!(
        !s1.account_exists("alice@example.com"),
        "migrate should have revoked B on the old server"
    );
    let status = a.run(&["account", "status"]);
    assert!(
        status.contains(&s2.base_url),
        "status should now point at s2:\n{status}"
    );
    assert!(
        !status.contains(&s1.base_url),
        "status should no longer reference the old server:\n{status}"
    );
}

/// AC #6: a second device that logs in to the account then re-adopts continues
/// only when the crypto ids (A and V) match — an idempotent success here, since
/// the second device materialised its vault from the same server header.
#[test]
fn second_device_readopt_succeeds_when_ids_match() {
    let server = TestServer::start();
    let dir = tempfile::tempdir().expect("work dir");
    let a = Device::new(dir.path(), "a");
    let b = Device::new(dir.path(), "b");

    // Device A: adopt an existing local vault and push it.
    let secret_key = a.init_vault("SharedTaskZZ");
    a.run(&[
        "account",
        "adopt",
        "--server",
        &server.base_url,
        "--email",
        "alice@example.com",
    ]);
    a.run(&["sync"]);

    // Device B: log in with the account's Secret Key (materialises the vault
    // from the server header — so B's A and V equal A's).
    b.set_secret_key(secret_key);
    b.run(&[
        "account",
        "login",
        "--server",
        &server.base_url,
        "--email",
        "alice@example.com",
    ]);
    b.run(&["sync", "--server", &server.base_url]);
    assert!(
        b.titles().iter().any(|t| t == "SharedTaskZZ"),
        "B should pull A's task"
    );

    // B re-adopts: register returns 409, so B logs in, fetches the header, and
    // continues because A and V match (the idempotent second-device path).
    let out = b.run(&[
        "account",
        "adopt",
        "--server",
        &server.base_url,
        "--email",
        "alice@example.com",
    ]);
    assert!(
        out.contains("Adopted"),
        "second-device adopt should succeed:\n{out}"
    );
    assert_eq!(
        b.meta_id("vault_id"),
        a.meta_id("vault_id"),
        "second device V must match"
    );
    assert_eq!(
        b.meta_id("account_id"),
        a.meta_id("account_id"),
        "second device A must match"
    );
}

/// AC #7: `disconnect` revokes the server principal B (the account is gone from
/// the server), clears credentials (status → not signed in), preserves all local
/// tasks, and leaves the vault re-adoptable.
#[test]
fn disconnect_revokes_principal_preserves_data_and_re_adopts() {
    let server = TestServer::start();
    let dir = tempfile::tempdir().expect("work dir");
    let a = Device::new(dir.path(), "a");

    a.init_vault("KeepMeZZ");
    a.run(&[
        "account",
        "adopt",
        "--server",
        &server.base_url,
        "--email",
        "alice@example.com",
    ]);
    a.run(&["sync"]);
    assert!(
        server.account_exists("alice@example.com"),
        "adopt should register B"
    );

    // Disconnect: B is revoked (the account row is deleted) and credentials go.
    let out = a.run(&["account", "disconnect"]);
    assert!(
        out.contains("Disconnected"),
        "disconnect should confirm:\n{out}"
    );
    assert!(
        !server.account_exists("alice@example.com"),
        "disconnect must revoke the server principal B"
    );
    let status = a.run(&["account", "status"]);
    assert!(
        status.contains("Not signed in"),
        "status should show signed out after disconnect:\n{status}"
    );

    // Local data is preserved and the vault stays usable.
    assert!(
        a.titles().iter().any(|t| t == "KeepMeZZ"),
        "local task must survive disconnect"
    );

    // The vault is re-adoptable afterward (bucket freed, LocalOnly again).
    let re = a.run(&[
        "account",
        "adopt",
        "--server",
        &server.base_url,
        "--email",
        "alice@example.com",
    ]);
    assert!(
        re.contains("Adopted"),
        "re-adopt after disconnect should succeed:\n{re}"
    );
    assert!(server.account_exists("alice@example.com"));
}
