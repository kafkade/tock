//! Integration tests for the first-run LOCAL onboarding gate (issue #198).
//!
//! These spawn the real `tock` binary in non-interactive (no-TTY) mode — the
//! same way scripts and CI drive it — via `TOCK_VAULT` / `TOCK_PASSWORD` /
//! `TOCK_SECRET_KEY`, with `TOCK_NO_KEYRING=1` so the credential cache uses the
//! isolated file fallback and never touches the developer's real OS keyring.
//!
//! Coverage maps to the acceptance criteria:
//! * **AC #1** — an empty (or absent, non-interactive) password is rejected and
//!   no vault is created, instead of silently defaulting to empty.
//! * **AC #3** — the Emergency Kit / Secret Key is shown exactly once (on first
//!   init), never again on later commands.
//! * **AC #4** — the Secret Key is cached and transparently reused so later
//!   commands open the vault without re-supplying it.
//! * **AC #6** — scripted mode: a non-empty `TOCK_PASSWORD` is accepted without
//!   any prompt (no hang), and the missing-password case fails loudly.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::Command;

/// A reasonably strong scripted password used across the success-path tests.
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

    /// A `tock` command with a fully isolated, deterministic environment and no
    /// inherited password / Secret Key.
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

    /// Where the file-fallback credential cache lands (`TOCK_NO_KEYRING=1`).
    fn creds_file(&self) -> PathBuf {
        self.dir
            .path()
            .join(".config")
            .join("tock")
            .join("credentials.json")
    }
}

fn find_token(output: &str, prefix: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|tok| tok.starts_with(prefix))
        .map(str::to_string)
}

/// AC #1 / AC #6: an explicitly empty password is rejected and no vault is
/// created — the gate never silently produces an empty-password vault.
#[test]
fn empty_password_is_rejected_and_no_vault_created() {
    let sb = Sandbox::new();
    let out = sb
        .command()
        .env("TOCK_PASSWORD", "")
        .args(["add", "hello"])
        .output()
        .expect("spawn tock");
    assert!(
        !out.status.success(),
        "an empty password must be rejected on first run"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("empty password"),
        "expected a clear empty-password error, got:\n{stderr}"
    );
    assert!(
        !sb.vault.exists(),
        "no vault should be created when the password is rejected"
    );
}

/// AC #1 / AC #6: with no password and no TTY, the gate fails with an
/// actionable error rather than defaulting to empty.
#[test]
fn missing_password_non_interactive_errors_not_silent_empty() {
    let sb = Sandbox::new();
    let out = sb
        .command()
        .args(["add", "hello"])
        .output()
        .expect("spawn tock");
    assert!(
        !out.status.success(),
        "a missing password in non-interactive mode must fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("empty password") && stderr.contains("--password"),
        "expected an actionable error pointing at --password, got:\n{stderr}"
    );
    assert!(!sb.vault.exists(), "no vault should be created");
}

/// AC #3 / AC #6: a scripted non-empty password creates the vault without any
/// prompt and shows the Emergency Kit exactly once.
#[test]
fn scripted_password_creates_vault_and_shows_kit_once() {
    let sb = Sandbox::new();

    let out = sb
        .command()
        .env("TOCK_PASSWORD", PASSWORD)
        .args(["add", "buy milk"])
        .output()
        .expect("spawn tock");
    assert!(
        out.status.success(),
        "first-run init should succeed in scripted mode:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf8 stdout");
    assert!(
        stdout.contains("EMERGENCY KIT"),
        "the Emergency Kit must be shown on first run:\n{stdout}"
    );
    let secret_key = find_token(&stdout, "A4-").expect("an `A4-` Secret Key is printed once");
    assert!(sb.vault.exists(), "the vault file should now exist");

    // A later command with the Secret Key supplied must NOT reprint the kit.
    let out2 = sb
        .command()
        .env("TOCK_PASSWORD", PASSWORD)
        .env("TOCK_SECRET_KEY", &secret_key)
        .args(["list"])
        .output()
        .expect("spawn tock");
    assert!(
        out2.status.success(),
        "opening the vault should succeed:\n{}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let stdout2 = String::from_utf8_lossy(&out2.stdout);
    assert!(
        !stdout2.contains("EMERGENCY KIT") && !stdout2.contains("A4-"),
        "the Emergency Kit / Secret Key must be shown only once:\n{stdout2}"
    );
}

/// AC #4: the Secret Key is cached at first run and transparently reused, so a
/// later command opens the vault without `--secret-key` / `TOCK_SECRET_KEY`.
#[test]
fn secret_key_is_cached_and_reused_on_open() {
    let sb = Sandbox::new();

    let out = sb
        .command()
        .env("TOCK_PASSWORD", PASSWORD)
        .args(["add", "task one"])
        .output()
        .expect("spawn tock");
    assert!(
        out.status.success(),
        "first-run init should succeed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        sb.creds_file().exists(),
        "the Secret Key should be cached to the credential file"
    );

    // No Secret Key supplied: it must be loaded from the cache to open.
    let out2 = sb
        .command()
        .env("TOCK_PASSWORD", PASSWORD)
        .args(["--format", "json", "list"])
        .output()
        .expect("spawn tock");
    assert!(
        out2.status.success(),
        "opening via the cached Secret Key should succeed:\n{}",
        String::from_utf8_lossy(&out2.stderr)
    );
}
