//! `tock context` — manage named filter contexts.

use clap::{Args, Subcommand};

/// Context management arguments.
#[derive(Debug, Args)]
pub struct ContextArgs {
    /// Context subcommand.
    #[command(subcommand)]
    pub command: ContextCommand,
}

/// Context subcommands.
#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    /// Activate a context.
    Set {
        /// Context name.
        name: String,
    },
    /// Clear the active context.
    Clear,
    /// Define a new context.
    Define {
        /// Context name.
        name: String,
        /// Filter expression.
        filter: String,
    },
    /// List all contexts.
    #[command(alias = "ls")]
    List,
    /// Delete a context.
    Rm {
        /// Context name.
        name: String,
    },
}
