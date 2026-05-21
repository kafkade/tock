//! Time tracking commands.

use clap::{Args, Subcommand};

/// Arguments for the `time` command group.
#[derive(Debug, Args)]
pub struct TimeArgs {
    /// Time subcommand to execute.
    #[command(subcommand)]
    pub command: TimeCommand,
}

/// Time tracking subcommands.
#[derive(Debug, Subcommand)]
pub enum TimeCommand {
    /// Start timing. Creates a block immediately.
    Start {
        /// Description words, or a task SID followed by optional text.
        #[arg(trailing_var_arg = true, num_args = 1..)]
        words: Vec<String>,
    },
    /// Stop the current timer.
    Stop,
    /// Resume the most recently stopped timer.
    Resume,
    /// Show the currently running timer.
    Current,
    /// List time blocks.
    Blocks {
        /// Period: today, week, month, all.
        #[arg(default_value = "today")]
        period: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Time report summary.
    Report {
        /// Period: today, week, month.
        #[arg(default_value = "today")]
        period: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Edit a time block.
    Edit {
        /// Time block SID.
        sid: u32,
        /// New title.
        #[arg(long)]
        title: Option<String>,
        /// New notes.
        #[arg(long)]
        notes: Option<String>,
        /// New start time (ISO 8601).
        #[arg(long)]
        start: Option<String>,
        /// New end time (ISO 8601).
        #[arg(long)]
        end: Option<String>,
        /// Link to task SID.
        #[arg(long)]
        task: Option<u32>,
        /// Mark as billable.
        #[arg(long)]
        billable: Option<bool>,
    },
}
