//! `cargo xtask i18n-check` — validate the Fluent localization catalogs.
//!
//! Ensures every locale parses and that each non-reference locale defines
//! exactly the same set of message and term ids as the reference locale
//! (`en-US`). This guards against drift: a translation that is missing strings
//! (untranslated UI) or has stray ids (typos / removed strings) fails CI.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use fluent_syntax::ast;
use fluent_syntax::parser;

/// Reference locale whose id set every other locale must match.
const REFERENCE_LOCALE: &str = "en-US";

/// Relative path (from the workspace root) to the localization assets.
const ASSETS_DIR: &str = "crates/tock-cli/i18n";

/// Run the i18n catalog check, returning a human-readable error on failure.
pub fn run() -> Result<(), String> {
    let assets = workspace_root().join(ASSETS_DIR);
    let locales = discover_locales(&assets)?;

    if !locales.iter().any(|l| l.name == REFERENCE_LOCALE) {
        return Err(format!(
            "reference locale `{REFERENCE_LOCALE}` not found under {}",
            assets.display()
        ));
    }

    let mut problems = String::new();
    let mut reference_ids: BTreeSet<String> = BTreeSet::new();

    // First pass: parse every locale and collect ids; record parse errors.
    let mut id_sets: Vec<(String, BTreeSet<String>)> = Vec::new();
    for locale in &locales {
        match collect_ids(&locale.files) {
            Ok(ids) => {
                if locale.name == REFERENCE_LOCALE {
                    reference_ids.clone_from(&ids);
                }
                id_sets.push((locale.name.clone(), ids));
            }
            Err(err) => {
                let _ = writeln!(problems, "[{}] {err}", locale.name);
            }
        }
    }

    // Second pass: compare each non-reference locale to the reference.
    for (name, ids) in &id_sets {
        if name == REFERENCE_LOCALE {
            continue;
        }
        let missing: Vec<&String> = reference_ids.difference(ids).collect();
        let extra: Vec<&String> = ids.difference(&reference_ids).collect();
        if !missing.is_empty() {
            let _ = writeln!(
                problems,
                "[{name}] missing {} id(s) present in {REFERENCE_LOCALE}: {}",
                missing.len(),
                join(&missing)
            );
        }
        if !extra.is_empty() {
            let _ = writeln!(
                problems,
                "[{name}] {} unknown id(s) not in {REFERENCE_LOCALE}: {}",
                extra.len(),
                join(&extra)
            );
        }
    }

    if problems.is_empty() {
        println!(
            "i18n-check: OK — {} locale(s), {} message/term id(s) in {REFERENCE_LOCALE}.",
            locales.len(),
            reference_ids.len()
        );
        Ok(())
    } else {
        Err(problems.trim_end().to_owned())
    }
}

/// A locale directory and its `.ftl` files.
struct Locale {
    name: String,
    files: Vec<PathBuf>,
}

/// Discover locale subdirectories (each holding `.ftl` files) under `assets`.
fn discover_locales(assets: &Path) -> Result<Vec<Locale>, String> {
    let entries = std::fs::read_dir(assets)
        .map_err(|e| format!("cannot read assets dir {}: {e}", assets.display()))?;

    let mut locales = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read dir entry: {e}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(ToOwned::to_owned)
        else {
            continue;
        };
        let mut files = Vec::new();
        let inner = std::fs::read_dir(&path)
            .map_err(|e| format!("cannot read locale dir {}: {e}", path.display()))?;
        for f in inner {
            let f = f.map_err(|e| format!("cannot read locale entry: {e}"))?;
            let fp = f.path();
            if fp.extension().and_then(|e| e.to_str()) == Some("ftl") {
                files.push(fp);
            }
        }
        files.sort();
        if !files.is_empty() {
            locales.push(Locale { name, files });
        }
    }
    locales.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(locales)
}

/// Parse all `.ftl` files for a locale and collect message and term ids.
///
/// Terms are prefixed with `-` to distinguish them from messages, matching
/// Fluent's own reference syntax. Returns an error string if any file fails to
/// parse or defines a duplicate id.
fn collect_ids(files: &[PathBuf]) -> Result<BTreeSet<String>, String> {
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for file in files {
        let source = std::fs::read_to_string(file)
            .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let resource = parser::parse(source.as_str()).map_err(|(_, errors)| {
            let detail = errors
                .iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join("; ");
            format!("parse error in {}: {detail}", file.display())
        })?;

        for entry in &resource.body {
            let id = match entry {
                ast::Entry::Message(m) => m.id.name.to_string(),
                ast::Entry::Term(t) => format!("-{}", t.id.name),
                _ => continue,
            };
            if !ids.insert(id.clone()) {
                return Err(format!("duplicate id `{id}` in {}", file.display()));
            }
        }
    }
    Ok(ids)
}

/// Join a list of ids into a stable, comma-separated string for messages.
fn join(ids: &[&String]) -> String {
    ids.iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Resolve the workspace root from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}
