//! `tock tag` — tag management commands.

use clap::{Args, Subcommand};

/// Tag management arguments.
#[derive(Debug, Args)]
pub struct TagArgs {
    /// Tag subcommand.
    #[command(subcommand)]
    pub command: TagCommand,
}

/// Tag subcommands.
#[derive(Debug, Subcommand)]
pub enum TagCommand {
    /// List all tags.
    #[command(alias = "ls")]
    List,
    /// Rename a tag (updates all tagged entities).
    Rename {
        /// Current tag name.
        old: String,
        /// New tag name.
        new: String,
    },
}
