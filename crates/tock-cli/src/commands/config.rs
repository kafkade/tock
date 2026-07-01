//! `tock config` — inspect and scaffold the user configuration file.

use clap::{Args, Subcommand};

/// Configuration management arguments.
#[derive(Debug, Args)]
pub struct ConfigArgs {
    /// Config subcommand.
    #[command(subcommand)]
    pub command: ConfigCommand,
}

/// Configuration subcommands.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show the effective (merged) configuration.
    Show {
        /// Emit JSON instead of TOML.
        #[arg(long)]
        json: bool,
    },
    /// Print the resolved configuration file path.
    Path,
    /// Write a documented sample config file if one does not exist.
    Init {
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
}
