//! Focus timer commands.

use clap::{Args, Subcommand};

/// Arguments for the `focus` command group.
#[derive(Debug, Args)]
pub struct FocusArgs {
    /// Focus subcommand to execute.
    #[command(subcommand)]
    pub command: FocusCommand,
}

/// Focus timer subcommands.
#[derive(Debug, Subcommand)]
pub enum FocusCommand {
    /// Start a focus session (default: 4 cycles of 25/5/15).
    Start {
        /// Task SID to focus on (optional).
        #[arg(short, long)]
        task: Option<u32>,
        /// Number of Pomodoro cycles (default: 4).
        #[arg(short, long, default_value = "4")]
        cycles: u32,
        /// Work interval in minutes (default: 25).
        #[arg(long, default_value = "25")]
        work: u32,
        /// Short break in minutes (default: 5).
        #[arg(long, default_value = "5")]
        short_break: u32,
        /// Long break in minutes (default: 15).
        #[arg(long, default_value = "15")]
        long_break: u32,
    },
    /// Complete the current work interval (mark a pomodoro done).
    Done,
    /// Skip the current break and start working.
    SkipBreak,
    /// Pause the current session.
    Pause,
    /// Resume a paused session.
    Resume,
    /// Abort the current session.
    Stop,
    /// Show the current focus session status.
    Status,
    /// Show focus stats for a period.
    Stats {
        /// Period: today, week, month (default: today).
        #[arg(default_value = "today")]
        period: String,
    },
    /// Show focus session history for a task.
    History {
        /// Task SID.
        task: u32,
    },
}
