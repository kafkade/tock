//! End-to-end **cross-server migration** acceptance test (issue #202).
//!
//! Spins up **two** real `tock-server` instances in-process and drives actual
//! `tock` CLI binaries through the full "move my account to another server"
//! story, proving the sequence that genuinely works:
//!
//! ```text
//! tock account export                       # authenticated to server 1
//! tock account adopt --server S2 --migrate  # register + claim V on server 2
//! tock account import <archive>             # authenticated to server 2
//! tock sync
//! ```
//!
//! Two things this test pins down that are easy to get wrong:
//!
//! 1. **`adopt --migrate` alone does not move history.** `adopt` pushes
//!    `collect_local_changes`, which is a *journal diff* — a vault that already
//!    synced to server 1 produces zero events, so server 2 receives only the
//!    wrapped header. The negative control below asserts server 2's event log is
//!    **empty** right after the migrate, which is exactly why `import` exists.
//! 2. **The order cannot be export → import → adopt.** `import` requires a live
//!    SRP session on the destination server, and only `adopt` creates one; while
//!    `export` must precede `--migrate`, which tears down the *old* server's
//!    credentials as its first act.
//!
//! It also asserts the crypto identity survives the move: the client-minted
//! `account_id` (**A**) and `vault_id` (**V**) are byte-identical on both
//! servers (ADR-016 §1), and server 2 still stores only ciphertext.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
// The migration test is deliberately one linear, readable narrative.
#![allow(clippy::too_many_lines)]

mod common;

use std::path::Path;

use common::{Device, TestServer, assert_no_plaintext, signup};

/// The subset of the ciphertext archive this test inspects. Declared locally
/// because the CLI is treated as a black box (and `tock-cli` has no lib target).
#[derive(serde::Deserialize)]
struct Archive {
    vault_id: String,
    #[serde(default)]
    header: Option<String>,
    events: Vec<ArchiveEvent>,
}

#[derive(serde::Deserialize)]
struct ArchiveEvent {
    event_id: String,
}

impl Archive {
    fn read(path: &Path) -> Self {
        let raw =
            std::fs::read(path).unwrap_or_else(|e| panic!("read archive {}: {e}", path.display()));
        serde_json::from_slice(&raw)
            .unwrap_or_else(|e| panic!("parse archive {}: {e}", path.display()))
    }

    /// The client crypto `account_id` (**A**) carried inside the non-secret
    /// vault header, as lowercase hex.
    fn account_id(&self) -> String {
        let header = self.header.as_deref().expect("archive carries a header");
        let bytes = base64_decode(header).expect("header is base64");
        let parsed = tock_core::vault::VaultHeader::from_bytes(&bytes).expect("parse vault header");
        parsed.account_id.simple().to_string()
    }

    /// The **V** recorded inside the header, which must agree with the
    /// archive's own `vault_id` field.
    fn header_vault_id(&self) -> String {
        let header = self.header.as_deref().expect("archive carries a header");
        let bytes = base64_decode(header).expect("header is base64");
        let parsed = tock_core::vault::VaultHeader::from_bytes(&bytes).expect("parse vault header");
        parsed.vault_id.simple().to_string()
    }

    fn event_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.events.iter().map(|e| e.event_id.clone()).collect();
        ids.sort();
        ids
    }
}

/// Standard-alphabet base64 decode (the archive encoding).
fn base64_decode(s: &str) -> Option<Vec<u8>> {
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
            b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

/// The full migration: server 1 → export → `adopt --migrate` → import into
/// server 2 → the vault (and a *second* device) works against server 2 with the
/// crypto identity A/V preserved.
#[test]
fn account_migrates_between_servers_preserving_identity_and_history() {
    let s1 = TestServer::start();
    let s2 = TestServer::start();
    let dir = tempfile::tempdir().expect("work dir");
    let a = Device::new(dir.path(), "a");

    // Distinctive plaintext markers; none may appear in either server's blobs.
    let markers = ["MigrateAlphaZZ", "MigrateBetaZZ", "PostMoveZZ"];

    // ── Server 1: account, data, full sync ───────────────────────────
    let account = signup(&s1.base_url, &a, "alice@example.com");
    a.run(&["add", "MigrateAlphaZZ"]);
    a.run(&["add", "MigrateBetaZZ"]);
    a.run(&["sync", "--server", &s1.base_url]);

    // ── Step 1: export (authenticated to server 1) ───────────────────
    let archive_path = dir.path().join("vault.json");
    let archive_str = archive_path.to_str().expect("utf8 path");
    let export_out = a.run(&["account", "export", "--out", archive_str]);
    assert!(
        export_out.contains("CIPHERTEXT ONLY") && export_out.contains("Emergency Kit"),
        "export must state the archive is ciphertext needing the Emergency Kit:\n{export_out}"
    );

    let from_s1 = Archive::read(&archive_path);
    assert!(
        !from_s1.events.is_empty(),
        "server 1 archive should carry the synced event log"
    );
    assert_eq!(
        from_s1.vault_id,
        from_s1.header_vault_id(),
        "archive vault_id must match the header's V"
    );

    // ── Step 2: adopt --migrate onto server 2 ────────────────────────
    let migrate_out = a.run(&[
        "account",
        "adopt",
        "--server",
        &s2.base_url,
        "--email",
        "alice@example.com",
        "--migrate",
    ]);
    assert!(
        migrate_out.contains("Adopted local vault into"),
        "migrate should report adoption:\n{migrate_out}"
    );
    // The F1 footgun must be surfaced at the moment it becomes true: the binding
    // moved but the history did not, so a new device here would see an empty
    // vault until the archive is imported.
    assert!(
        migrate_out.contains("Binding moved:")
            && migrate_out.contains("event history did NOT move")
            && migrate_out.contains("tock account import"),
        "migrate must warn that history did not move and point at import:\n{migrate_out}"
    );

    // ── Negative control: the migrate moved NO history ───────────────
    // `adopt` pushes only journal-diff deltas, and a fully-synced vault has
    // none — so server 2 holds the header and an empty log. This is precisely
    // why `import` is load-bearing rather than redundant.
    let pre_import_path = dir.path().join("s2-pre-import.json");
    a.run(&[
        "account",
        "export",
        "--out",
        pre_import_path.to_str().expect("utf8 path"),
    ]);
    let pre_import = Archive::read(&pre_import_path);
    assert!(
        pre_import.events.is_empty(),
        "adopt --migrate must not be expected to move history; server 2 had {} event(s) \
         before import — if this ever changes, the documented migration sequence needs revisiting",
        pre_import.events.len()
    );
    assert!(
        pre_import.header.is_some(),
        "adopt should have uploaded the wrapped header to server 2"
    );

    // ── Step 3: import into server 2 ─────────────────────────────────
    let import_out = a.run(&["account", "import", archive_str]);
    assert!(
        import_out.contains("Imported") && import_out.contains("event(s)"),
        "import should report what it stored:\n{import_out}"
    );
    assert!(
        !import_out.contains("Imported 0 event(s)"),
        "import must actually move the event log:\n{import_out}"
    );

    // Idempotent: a second import stores nothing new.
    let second_import = a.run(&["account", "import", archive_str]);
    assert!(
        second_import.contains("Imported 0 event(s)"),
        "import must be idempotent:\n{second_import}"
    );

    // ── Step 4: sync against server 2 ────────────────────────────────
    let sync_out = a.run(&["sync"]);
    assert!(
        sync_out.contains(&s2.base_url),
        "sync should now target server 2:\n{sync_out}"
    );
    assert!(
        sync_out.contains("conflicts 0"),
        "re-pulling this device's own imported events must not conflict:\n{sync_out}"
    );
    // Local data is untouched by the move.
    let titles = a.title_status();
    assert!(
        titles.contains_key("MigrateAlphaZZ") && titles.contains_key("MigrateBetaZZ"),
        "migration must not lose local tasks, got: {titles:?}"
    );

    // ── Identity preserved: A and V are byte-identical on both servers ─
    let after_path = dir.path().join("s2-post-import.json");
    a.run(&[
        "account",
        "export",
        "--out",
        after_path.to_str().expect("utf8 path"),
    ]);
    let from_s2 = Archive::read(&after_path);
    assert_eq!(
        from_s2.vault_id, from_s1.vault_id,
        "vault_id (V) must survive the migration unchanged"
    );
    assert_eq!(
        from_s2.account_id(),
        from_s1.account_id(),
        "client crypto account_id (A) must survive the migration unchanged"
    );
    assert_eq!(
        from_s2.event_ids(),
        from_s1.event_ids(),
        "server 2 must hold exactly the event log exported from server 1"
    );

    // ── The vault really works against server 2 ──────────────────────
    // A brand-new device signs in to server 2 only, materialises the vault from
    // the header stored there, and must see the PRE-migration history. That is
    // only possible because `import` moved the event log.
    let b = Device::new(dir.path(), "b");
    b.set_secret_key(account.secret_key);
    b.run(&[
        "account",
        "login",
        "--server",
        &s2.base_url,
        "--email",
        "alice@example.com",
    ]);
    b.run(&["sync", "--server", &s2.base_url]);
    let b_titles = b.title_status();
    assert!(
        b_titles.contains_key("MigrateAlphaZZ") && b_titles.contains_key("MigrateBetaZZ"),
        "a fresh device on server 2 must see the pre-migration history, got: {b_titles:?}"
    );

    // Ongoing sync still works both ways after the move.
    a.run(&["add", "PostMoveZZ"]);
    a.run(&["sync"]);
    b.run(&["sync"]);
    assert!(
        b.title_status().contains_key("PostMoveZZ"),
        "post-migration changes must still propagate through server 2"
    );

    // ── Server 2 stores ciphertext only ──────────────────────────────
    let blobs = s2.stored_blobs();
    assert!(!blobs.is_empty(), "server 2 stored no events");
    assert_no_plaintext(&blobs, &markers);
}

/// Importing an archive that belongs to a **different** vault is refused
/// client-side, so foreign events can never be injected into your bucket.
#[test]
fn import_refuses_an_archive_from_another_vault() {
    let server = TestServer::start();
    let dir = tempfile::tempdir().expect("work dir");
    let a = Device::new(dir.path(), "a");

    signup(&server.base_url, &a, "alice@example.com");
    a.run(&["add", "MineZZ"]);
    a.run(&["sync", "--server", &server.base_url]);

    let path = dir.path().join("foreign.json");
    let path_str = path.to_str().expect("utf8 path");
    a.run(&["account", "export", "--out", path_str]);

    // Rewrite the archive so it claims to belong to some other vault.
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read archive"))
            .expect("parse archive");
    value["vault_id"] = serde_json::Value::String("ffffffffffffffffffffffffffffffff".into());
    std::fs::write(&path, serde_json::to_vec(&value).expect("serialize")).expect("write archive");

    let stderr = a.run_expecting_failure(&["account", "import", path_str]);
    assert!(
        stderr.contains("refusing to import another vault"),
        "expected a vault-mismatch refusal, got stderr:\n{stderr}"
    );
}

/// `tock account export` needs a server binding: on a purely local vault it
/// fails with actionable guidance rather than silently doing nothing.
#[test]
fn export_without_a_server_binding_is_an_error() {
    let dir = tempfile::tempdir().expect("work dir");
    let solo = Device::new(dir.path(), "solo");

    // Create a local-only vault (no signup, no adopt) and capture its key.
    let init_out = solo.run(&["add", "LocalOnlyZZ"]);
    let secret_key = common::find_token(&init_out, "A4-")
        .unwrap_or_else(|| panic!("no `A4-` Secret Key in init output:\n{init_out}"));
    solo.set_secret_key(secret_key);

    let stderr = solo.run_expecting_failure(&["account", "export"]);
    assert!(
        stderr.contains("this vault is local-only"),
        "expected guidance about the missing server binding, got stderr:\n{stderr}"
    );
}

/// `import` must offer **no** `--server` override.
///
/// Import is a *write* that makes the destination server claim this `vault_id`
/// (`ensure_vault` + `claim_vault_for_account`). An ad-hoc target would upload V's
/// history to a server the vault is not bound to while the local binding still
/// points elsewhere — the split-brain the authoritative-server invariant forbids
/// (ADR-016 §4). Rebinding is `adopt --migrate`'s job alone.
#[test]
fn import_has_no_server_override() {
    let dir = tempfile::tempdir().expect("work dir");
    let solo = Device::new(dir.path(), "solo");

    let help = solo.run(&["account", "import", "--help"]);
    assert!(
        !help.contains("--server"),
        "import must not expose a --server override:\n{help}"
    );

    // And the flag is genuinely rejected, not merely undocumented.
    let stderr = solo.run_expecting_failure(&[
        "account",
        "import",
        "archive.json",
        "--server",
        "https://elsewhere.example.com",
    ]);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("--server"),
        "passing --server to import should be a parse error, got stderr:\n{stderr}"
    );
}
