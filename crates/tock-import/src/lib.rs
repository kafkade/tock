//! # tock-import
//!
//! Import data into tock from portable formats.
//!
//! Supported formats:
//! - `json` — tock's own JSON export format
//! - `taskwarrior` — Taskwarrior `task export` JSON
//! - `things3` — Things 3 canonical JSON export
//! - `csv` — CSV files with auto-detected or TOML-mapped columns

pub mod csv_import;
pub mod json;
pub mod taskwarrior;
pub mod things3;
