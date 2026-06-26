//! Hook scripts API.
//!
//! Hooks are external scripts invoked at lifecycle events. They receive JSON on
//! stdin and can modify it (for pre-hooks) or observe it (for post-hooks). A
//! non-zero exit code from a pre-hook cancels the operation.
//!
//! Hook scripts live in `~/.config/tock/hooks/` and are named by event:
//! `on-add`, `on-modify`, `on-complete`, `on-delete`, etc.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Supported hook events.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug)]
pub enum HookEvent {
    /// Before a task is added.
    OnAdd,
    /// Before a task is modified.
    OnModify,
    /// After a task is completed.
    OnComplete,
    /// Before a task is deleted.
    OnDelete,
    /// After a habit log entry is recorded.
    OnHabitLog,
    /// When a timer starts.
    OnTimerStart,
    /// When a timer stops.
    OnTimerStop,
    /// When a focus session starts.
    OnFocusStart,
    /// When a focus session ends.
    OnFocusEnd,
}

impl HookEvent {
    /// Script filename for this event.
    #[must_use]
    pub const fn script_name(self) -> &'static str {
        match self {
            Self::OnAdd => "on-add",
            Self::OnModify => "on-modify",
            Self::OnComplete => "on-complete",
            Self::OnDelete => "on-delete",
            Self::OnHabitLog => "on-habit-log",
            Self::OnTimerStart => "on-timer-start",
            Self::OnTimerStop => "on-timer-stop",
            Self::OnFocusStart => "on-focus-start",
            Self::OnFocusEnd => "on-focus-end",
        }
    }
}

/// Directory where hook scripts are stored.
#[must_use]
pub fn hooks_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tock")
        .join("hooks")
}

/// Run a hook if the script exists.
///
/// Returns the hook's modified JSON, the original JSON when the hook produced
/// no stdout, or `None` if the hook cancelled the operation.
#[must_use]
#[allow(clippy::cognitive_complexity)]
pub fn run_hook(event: HookEvent, input_json: &str) -> Option<String> {
    // No hook installed for this event: pass the input through unchanged.
    // `None` is reserved for an installed hook that *cancels* the operation.
    let Some(script_path) = hook_script_path(event) else {
        return Some(input_json.to_string());
    };

    tracing::debug!(hook = event.script_name(), path = %script_path.display(), "running hook");

    let output = spawn_hook(&script_path, input_json);
    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.trim().is_empty() {
                Some(input_json.to_string())
            } else {
                Some(stdout.into_owned())
            }
        }
        Ok(out) => {
            tracing::warn!(hook = event.script_name(), code = ?out.status.code(), "hook cancelled operation");
            None
        }
        Err(error) => {
            tracing::warn!(hook = event.script_name(), error = %error, "hook execution failed");
            Some(input_json.to_string())
        }
    }
}

/// List all installed hook scripts.
#[must_use]
pub fn list_hooks() -> Vec<(HookEvent, PathBuf)> {
    let events = [
        HookEvent::OnAdd,
        HookEvent::OnModify,
        HookEvent::OnComplete,
        HookEvent::OnDelete,
        HookEvent::OnHabitLog,
        HookEvent::OnTimerStart,
        HookEvent::OnTimerStop,
        HookEvent::OnFocusStart,
        HookEvent::OnFocusEnd,
    ];

    events
        .into_iter()
        .filter_map(|event| hook_script_path(event).map(|path| (event, path)))
        .collect()
}

fn hook_script_path(event: HookEvent) -> Option<PathBuf> {
    let script = hooks_dir().join(event.script_name());
    [
        script.clone(),
        script.with_extension("sh"),
        script.with_extension("py"),
        script.with_extension("ps1"),
    ]
    .into_iter()
    .find(|candidate| candidate.exists())
}

fn spawn_hook(script_path: &Path, input_json: &str) -> std::io::Result<std::process::Output> {
    let mut command = build_command(script_path);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    if let Some(stdin) = &mut child.stdin {
        stdin.write_all(input_json.as_bytes())?;
    }

    child.wait_with_output()
}

fn build_command(script_path: &Path) -> Command {
    match script_path.extension().and_then(|ext| ext.to_str()) {
        Some("ps1") => {
            let mut command = Command::new("powershell");
            command
                .arg("-NoProfile")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(script_path);
            command
        }
        Some("py") => {
            let mut command = Command::new("python");
            command.arg(script_path);
            command
        }
        Some("sh") => {
            let mut command = Command::new("sh");
            command.arg(script_path);
            command
        }
        _ => Command::new(script_path),
    }
}
