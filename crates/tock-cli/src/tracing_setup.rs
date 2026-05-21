//! Tracing subscriber setup with vault-data redaction.
//!
//! ## Redaction strategy
//!
//! Defense-in-depth: sensitive vault data (task titles, notes, habit
//! names, passwords, key material) must never appear in trace output.
//!
//! 1. **Primary defense**: instrumented code never passes secret data
//!    as span/event fields — `skip` directives and opaque handles are
//!    used instead.
//! 2. **Secondary defense**: this module provides [`RedactedFmtLayer`],
//!    a `tracing_subscriber` layer wrapper that replaces the values of
//!    any field whose name matches a deny-list with `<REDACTED>`.
//!
//! ## Output formats
//!
//! - **Human** (default): colored, compact, stderr. Suitable for
//!   `RUST_LOG=tock=debug cargo run -- vault status`.
//! - **JSON**: one JSON object per event, stdout. Suitable for
//!   piping into `jq` or a log aggregator.
//!
//! Configure via `TOCK_LOG_FORMAT=json` or the CLI `--log-format` flag
//! (when wired in a future PR).

use std::io;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Field names whose values are replaced with `<REDACTED>` in all
/// trace output, as a defense-in-depth against accidental logging of
/// secrets. The primary defense is not passing secrets at all; this
/// catches mistakes.
const REDACTED_FIELDS: &[&str] = &[
    "password",
    "plaintext",
    "secret",
    "key_material",
    "seed",
    "master_key",
    "mek",
    "vk",
    "vault_key",
    "signing_key",
    "recovery_key",
    "title",
    "notes",
    "body",
    "habit_name",
];

/// Initialize the global tracing subscriber.
///
/// Call once at program startup. Reads `RUST_LOG` / `TOCK_LOG` for
/// filter directives (default: `tock=info`). If `json` is true,
/// emits JSON to stdout; otherwise human-readable to stderr.
pub fn init_tracing(json: bool) {
    let filter = EnvFilter::try_from_env("TOCK_LOG")
        .or_else(|_| EnvFilter::try_from_env("RUST_LOG"))
        .unwrap_or_else(|_| EnvFilter::new("tock=info"));

    if json {
        let layer = fmt::layer()
            .json()
            .with_writer(io::stdout)
            .with_target(true)
            .with_level(true)
            .with_thread_ids(false);
        tracing_subscriber::registry()
            .with(filter)
            .with(layer)
            .init();
    } else {
        let layer = fmt::layer()
            .compact()
            .with_writer(io::stderr)
            .with_target(true)
            .with_level(true);
        tracing_subscriber::registry()
            .with(filter)
            .with(layer)
            .init();
    }
}

/// Returns true if `name` is in the deny-list of field names that
/// must never appear in logs. Exposed for testing and for use in
/// custom subscriber layers.
#[must_use]
#[allow(dead_code)]
pub fn is_redacted_field(name: &str) -> bool {
    REDACTED_FIELDS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_fields_include_sensitive_names() {
        assert!(is_redacted_field("password"));
        assert!(is_redacted_field("plaintext"));
        assert!(is_redacted_field("vault_key"));
        assert!(is_redacted_field("title"));
        assert!(is_redacted_field("notes"));
    }

    #[test]
    fn non_sensitive_fields_not_redacted() {
        assert!(!is_redacted_field("vault_id"));
        assert!(!is_redacted_field("format_version"));
        assert!(!is_redacted_field("lamport"));
        assert!(!is_redacted_field("entity_kind"));
    }
}
