//! `tock report` — saved report commands.

use clap::{Args, Subcommand};

/// Report management arguments.
#[derive(Debug, Args)]
pub struct ReportArgs {
    /// Report subcommand.
    #[command(subcommand)]
    pub command: ReportCommand,
}

/// Report subcommands.
#[derive(Debug, Subcommand)]
pub enum ReportCommand {
    /// Define a new report.
    Define {
        /// Report name.
        name: String,
        /// Filter query (for example `status:pending tag:work`).
        #[arg(long)]
        query: String,
        /// Sort by field (`urgency`, `deadline`, `created`, `sid`).
        #[arg(long)]
        sort: Option<String>,
        /// Columns to display (`sid,priority,title,deadline,tags`).
        #[arg(long, default_value = "sid,priority,status,title,deadline,tags")]
        columns: String,
    },
    /// List all saved reports.
    #[command(alias = "ls")]
    List,
    /// Run a saved report.
    Show {
        /// Report name.
        name: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Delete a report.
    Rm {
        /// Report name.
        name: String,
    },
}
