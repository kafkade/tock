//! `tock uda` — user-defined attribute management commands.

use clap::{Args, Subcommand};

/// UDA management arguments.
#[derive(Debug, Args)]
pub struct UdaArgs {
    /// UDA subcommand.
    #[command(subcommand)]
    pub command: UdaCommand,
}

/// UDA subcommands.
#[derive(Debug, Subcommand)]
pub enum UdaCommand {
    /// Define a new UDA.
    Add {
        /// Attribute key name.
        key: String,
        /// Type: string, number, date, boolean.
        #[arg(long, default_value = "string")]
        r#type: String,
        /// Human-readable label.
        #[arg(long)]
        label: Option<String>,
        /// Default value.
        #[arg(long)]
        default: Option<String>,
    },
    /// List all defined UDAs.
    #[command(alias = "ls")]
    List,
    /// Remove a UDA definition.
    Rm {
        /// Key to remove.
        key: String,
    },
}
