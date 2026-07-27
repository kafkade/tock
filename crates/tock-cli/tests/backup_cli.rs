//! Integration tests for `tock backup create` / `tock backup restore`
//! (issue #200, ADR-018).
//!
//! These spawn the real `tock` binary in non-interactive mode via
//! `TOCK_VAULT` / `TOCK_PASSWORD` / `TOCK_SECRET_KEY`, with
//! `TOCK_NO_KEYRING=1` so the credential cache uses the isolated file
//! fallback and never touches the developer's real OS keyring.
//!
//! Coverage maps to the acceptance criteria:
//! * **AC #1 / AC #3-A** — round-trip: create an encrypted archive, delete
//!   the vault, restore it in disaster-recovery mode, and confirm the task
//!   data survives.
//! * **AC #3-B** — clone restore succeeds offline and prints the
//!   reconcile-before-push guidance.
//! * **AC #5** — `tock export` emits a loud plaintext / not-a-backup
//!   warning to stderr.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::Command;

const PASSWORD: &str = "correct-horse-9!";

const fn tock_bin() -> &'static str {
    env!("CARGO_BIN_EXE_tock")
}

/// An isolated vault + config sandbox for one test.
struct Sandbox {
    dir: tempfile::TempDir,
    vault: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = dir.path().join("tock.tockvault");
        Self { dir, vault }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(tock_bin());
        cmd.env("TOCK_VAULT", &self.vault)
            .env("HOME", self.dir.path())
            .env("USERPROFILE", self.dir.path())
            .env("XDG_CONFIG_HOME", self.dir.path().join(".config"))
            .env("TOCK_NO_KEYRING", "1")
            .env_remove("TOCK_PASSWORD")
            .env_remove("TOCK_SECRET_KEY")
            .env_remove("TOCK_SERVER");
        cmd
    }
}

fn find_token(output: &str, prefix: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|tok| tok.starts_with(prefix))
        .map(str::to_string)
}

/// First-run init that returns the printed Secret Key.
fn init_vault(sb: &Sandbox, title: &str) -> String {
    let out = sb
        .command()
        .env("TOCK_PASSWORD", PASSWORD)
        .args(["add", title])
        .output()
        .expect("spawn tock add");
    assert!(
        out.status.success(),
        "first-run init should succeed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf8 stdout");
    find_token(&stdout, "A4-").expect("an `A4-` Secret Key is printed once")
}

/// AC #1 / AC #3-A: create → delete vault → restore (disaster recovery) →
/// data is identical.
#[test]
fn create_then_disaster_recovery_restore_roundtrips() {
    let sb = Sandbox::new();
    let title = "backup me please";
    let secret_key = init_vault(&sb, title);

    let archive = sb.dir.path().join("snapshot.tockbak");

    // Create the encrypted backup.
    let out = sb
        .command()
        .env("TOCK_PASSWORD", PASSWORD)
        .env("TOCK_SECRET_KEY", &secret_key)
        .args(["backup", "create", "--out"])
        .arg(&archive)
        .output()
        .expect("spawn tock backup create");
    assert!(
        out.status.success(),
        "backup create should succeed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(archive.exists(), "the archive file should be written");

    // Simulate total loss of the device: delete the vault file.
    std::fs::remove_file(&sb.vault).expect("remove vault");
    assert!(!sb.vault.exists());

    // Restore in disaster-recovery mode.
    let out = sb
        .command()
        .env("TOCK_PASSWORD", PASSWORD)
        .env("TOCK_SECRET_KEY", &secret_key)
        .args(["backup", "restore"])
        .arg(&archive)
        .args(["--mode", "disaster-recovery"])
        .output()
        .expect("spawn tock backup restore");
    assert!(
        out.status.success(),
        "backup restore should succeed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(sb.vault.exists(), "the vault should be recreated");

    // The task must be back.
    let out = sb
        .command()
        .env("TOCK_PASSWORD", PASSWORD)
        .env("TOCK_SECRET_KEY", &secret_key)
        .args(["list"])
        .output()
        .expect("spawn tock list");
    assert!(
        out.status.success(),
        "list should succeed after restore:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(title),
        "the restored vault should still contain the task:\n{stdout}"
    );
}

/// AC #3-B: clone restore succeeds offline (mints a fresh identity, clears
/// the binding) and prints the reconcile-before-push guidance.
#[test]
fn clone_restore_succeeds_and_prints_guidance() {
    let sb = Sandbox::new();
    let title = "clone me";
    let secret_key = init_vault(&sb, title);

    let archive = sb.dir.path().join("snapshot.tockbak");
    let out = sb
        .command()
        .env("TOCK_PASSWORD", PASSWORD)
        .env("TOCK_SECRET_KEY", &secret_key)
        .args(["backup", "create", "--out"])
        .arg(&archive)
        .output()
        .expect("spawn tock backup create");
    assert!(out.status.success(), "backup create should succeed");

    // Restore onto a fresh path as a second device.
    let clone_vault = sb.dir.path().join("clone.tockvault");
    let out = sb
        .command()
        .env("TOCK_VAULT", &clone_vault)
        .env("TOCK_PASSWORD", PASSWORD)
        .env("TOCK_SECRET_KEY", &secret_key)
        .args(["backup", "restore"])
        .arg(&archive)
        .args(["--mode", "clone"])
        .output()
        .expect("spawn tock backup restore --mode clone");
    assert!(
        out.status.success(),
        "clone restore should succeed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("clone") && stdout.contains("tock sync"),
        "clone restore should print reconcile-before-push guidance:\n{stdout}"
    );

    // The cloned vault opens and still has the data.
    let out = sb
        .command()
        .env("TOCK_VAULT", &clone_vault)
        .env("TOCK_PASSWORD", PASSWORD)
        .env("TOCK_SECRET_KEY", &secret_key)
        .args(["list"])
        .output()
        .expect("spawn tock list");
    assert!(out.status.success(), "cloned vault should open");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(title),
        "cloned vault should contain the task"
    );
}

/// AC #5: `tock export json` prints a loud not-a-backup warning to stderr.
#[test]
fn export_prints_not_a_backup_warning() {
    let sb = Sandbox::new();
    let secret_key = init_vault(&sb, "exportable");

    let out = sb
        .command()
        .env("TOCK_PASSWORD", PASSWORD)
        .env("TOCK_SECRET_KEY", &secret_key)
        .args(["export", "json"])
        .output()
        .expect("spawn tock export json");
    assert!(
        out.status.success(),
        "export should succeed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("NOT a backup") && stderr.contains("tock backup create"),
        "export must warn that its output is not a backup:\n{stderr}"
    );
}
