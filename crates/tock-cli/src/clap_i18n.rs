//! Runtime localization of clap help text.
//!
//! clap's derive macro bakes English help (from doc-comments) into the
//! [`clap::Command`] at compile time. To localize it we walk the command tree
//! at runtime and replace each command's `about`/`long_about` and each
//! argument's help with a Fluent message, when one exists.
//!
//! ## Key scheme
//!
//! Keys are derived from the command path so translators never have to wire
//! anything up in Rust:
//!
//! - root about: `help-cli-about` (and `help-cli-long-about`)
//! - subcommand about: `help-cli-<name>-about`, nested as `help-cli-<a>-<b>-about`
//! - argument help: `help-cli-<path>-arg-<arg-id>`
//!
//! Because the loader always loads the `en-US` fallback, every key present in
//! the English catalog resolves even for incomplete translations, so clap help
//! is fully catalog-driven.

use clap::{Arg, Command};

use crate::i18n::loader;

/// Root key prefix for every clap help message.
const ROOT: &str = "help-cli";

/// Localize an entire clap [`Command`] tree in place.
#[must_use]
pub fn localize(command: Command) -> Command {
    localize_command(command, ROOT)
}

/// Look up a help message by key, returning `None` if it is not defined in any
/// loaded locale (including the `en-US` fallback).
fn message(key: &str) -> Option<String> {
    if loader().has(key) {
        Some(loader().get(key))
    } else {
        None
    }
}

/// Recursively localize a command and its subcommands at `path`.
fn localize_command(mut command: Command, path: &str) -> Command {
    if let Some(text) = message(&format!("{path}-about")) {
        command = command.about(text);
    }
    if let Some(text) = message(&format!("{path}-long-about")) {
        command = command.long_about(text);
    }

    let arg_ids: Vec<String> = command
        .get_arguments()
        .filter(|arg| !is_builtin_arg(arg))
        .map(|arg| arg.get_id().as_str().to_owned())
        .collect();
    for id in arg_ids {
        if let Some(text) = message(&format!("{path}-arg-{id}")) {
            command = command.mut_arg(&id, |arg| arg.help(text));
        }
    }

    let sub_names: Vec<String> = command
        .get_subcommands()
        .map(|sub| sub.get_name().to_owned())
        .collect();
    for name in sub_names {
        let child_path = format!("{path}-{name}");
        command = command.mut_subcommand(&name, |sub| localize_command(sub, &child_path));
    }

    command
}

/// clap injects `help`/`version` args automatically; skip them so we don't
/// emit or expect catalog keys for built-ins.
fn is_builtin_arg(arg: &Arg) -> bool {
    matches!(arg.get_id().as_str(), "help" | "version")
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;
    use crate::Cli;

    #[test]
    fn localize_is_idempotent_and_preserves_structure() {
        let original = Cli::command();
        let sub_before = original.get_subcommands().count();
        let localized = localize(Cli::command());
        assert_eq!(localized.get_subcommands().count(), sub_before);
    }

    /// Generates the `en-US` clap help catalog from the live command tree.
    ///
    /// Run with `cargo test -p tock-cli generate_clap_help_catalog -- --ignored
    /// --nocapture`; the output is written to `target/clap-help.en-US.ftl` for
    /// pasting into `i18n/en-US/tock-cli.ftl`.
    #[test]
    #[ignore = "generator utility, run manually"]
    fn generate_clap_help_catalog() {
        let mut out = String::new();
        dump_command(&Cli::command(), ROOT, &mut out);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/clap-help.en-US.ftl");
        if let Err(error) = std::fs::write(&path, out) {
            eprintln!("failed to write {}: {error}", path.display());
            return;
        }
        eprintln!("wrote {}", path.display());
    }

    fn ftl_value(key: &str, value: &str, out: &mut String) {
        use std::fmt::Write as _;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return;
        }
        if trimmed.contains('\n') {
            let _ = writeln!(out, "{key} =");
            for line in trimmed.lines() {
                let _ = writeln!(out, "    {line}");
            }
        } else {
            let _ = writeln!(out, "{key} = {trimmed}");
        }
    }

    fn dump_command(command: &Command, path: &str, out: &mut String) {
        use std::fmt::Write as _;
        let _ = writeln!(out, "\n# {path}");
        if let Some(about) = command.get_about() {
            ftl_value(&format!("{path}-about"), &about.to_string(), out);
        }
        if let Some(long) = command.get_long_about() {
            ftl_value(&format!("{path}-long-about"), &long.to_string(), out);
        }
        for arg in command.get_arguments() {
            if is_builtin_arg(arg) {
                continue;
            }
            if let Some(help) = arg.get_help() {
                let id = arg.get_id().as_str();
                ftl_value(&format!("{path}-arg-{id}"), &help.to_string(), out);
            }
        }
        for sub in command.get_subcommands() {
            let child = format!("{path}-{}", sub.get_name());
            dump_command(sub, &child, out);
        }
    }
}
