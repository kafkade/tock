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

mod common;

use common::{Device, TestServer, contains_bytes, find_token, login_with_setup_code, signup};

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

/// (ADR-018 §3B step 3) A clone restore must **reconcile remote history
/// before pushing**. This proves the one-shot `pending_reconcile` flag is
/// honored end-to-end: the clone's first `tock sync` pulls the server-only
/// task it never had *before* any push, and a second sync no longer
/// reconciles (the flag is one-shot).
#[test]
fn clone_restore_reconciles_before_push_then_clears_flag() {
    let server = TestServer::start();
    let dir = tempfile::tempdir().expect("work dir");
    let a = Device::new(dir.path(), "a");
    let clone = Device::new(dir.path(), "clone");

    // Device A: account + one task, pushed to the server.
    let signup = signup(&server.base_url, &a, "alice@example.com");
    a.run(&["add", "AlphaZZ"]);
    a.run(&["sync", "--server", &server.base_url]);

    // Back up A at this point (archive holds only Alpha).
    let archive = dir.path().join("clone.tockbak");
    let archive_str = archive.to_str().expect("utf8 path");
    a.run(&["backup", "create", "--out", archive_str]);

    // A then adds a SECOND task and pushes it — this is "newer remote
    // history" the clone's archive does not contain.
    a.run(&["add", "BetaZZ"]);
    a.run(&["sync"]);

    // Restore the archive as a CLONE onto a fresh device. It shares A's
    // account Secret Key, mints a new device id, and arms reconcile.
    clone.set_secret_key(signup.secret_key.clone());
    let restore_out = clone.run(&["backup", "restore", archive_str, "--mode", "clone"]);
    assert!(
        restore_out.contains("clone") && restore_out.contains("tock sync"),
        "clone restore should print reconcile guidance:\n{restore_out}"
    );
    // The clone starts with only Alpha (the archive's state).
    assert_eq!(
        clone.title_status().keys().cloned().collect::<Vec<_>>(),
        vec!["AlphaZZ".to_string()],
        "clone should start from the archive's single task"
    );

    // Reconnect this device to the server, then sync.
    login_with_setup_code(&clone, &signup);
    let first_sync = clone.run(&["sync", "--server", &server.base_url]);

    // Ordering proof: the reconcile pull is reported, and it precedes the
    // normal push/pull round in the same invocation.
    let reconcile_at = first_sync
        .find("Reconciled remote history before pushing")
        .unwrap_or_else(|| {
            panic!("clone's first sync must reconcile before pushing:\n{first_sync}")
        });
    let synced_at = first_sync.find("Synced with").unwrap_or_else(|| {
        panic!("clone's first sync must also run a normal round:\n{first_sync}")
    });
    assert!(
        reconcile_at < synced_at,
        "reconcile must run before the push/pull round:\n{first_sync}"
    );

    // The clone pulled the server-only task during reconcile.
    let titles = clone.title_status();
    assert!(
        titles.contains_key("AlphaZZ") && titles.contains_key("BetaZZ"),
        "clone should have reconciled BOTH tasks from the server, got: {titles:?}"
    );

    // One-shot: a second sync no longer reconciles (flag cleared).
    let second_sync = clone.run(&["sync"]);
    assert!(
        !second_sync.contains("Reconciled remote history before pushing"),
        "reconcile-before-push must be one-shot; second sync still reconciled:\n{second_sync}"
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
