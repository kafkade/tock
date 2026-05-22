//! `tock hooks` — manage hook scripts.

use clap::{Args, Subcommand};

/// Arguments for the `hooks` command group.
#[derive(Debug, Args)]
pub struct HooksArgs {
    /// Hook management subcommand.
    #[command(subcommand)]
    pub command: HooksCommand,
}

/// Hook management subcommands.
#[derive(Debug, Subcommand)]
pub enum HooksCommand {
    /// List installed hook scripts.
    #[command(alias = "ls")]
    List,
    /// Show the hooks directory path.
    Path,
}
