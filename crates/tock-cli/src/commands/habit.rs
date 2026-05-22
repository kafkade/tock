//! Habit tracking commands.

use clap::{Args, Subcommand};

/// Arguments for the `habit` command group.
#[derive(Debug, Args)]
pub struct HabitArgs {
    /// Habit subcommand to execute.
    #[command(subcommand)]
    pub command: HabitCommand,
}

/// Habit tracking subcommands.
#[derive(Debug, Subcommand)]
pub enum HabitCommand {
    /// Add a new habit.
    Add {
        /// Habit title.
        title: String,
        /// Identity statement (e.g. "I am a reader").
        #[arg(long)]
        identity: Option<String>,
        /// Cue (e.g. "After morning coffee").
        #[arg(long)]
        cue: Option<String>,
        /// Response (e.g. "Read 10 pages").
        #[arg(long)]
        response: Option<String>,
        /// Reward (e.g. "Enjoy tea").
        #[arg(long)]
        reward: Option<String>,
        /// Direction: build (default) or break.
        #[arg(long, default_value = "build")]
        direction: String,
        /// Cadence: daily (default), or JSON.
        #[arg(long, default_value = "\"daily\"")]
        cadence: String,
        /// Minimum: "boolean" (default), or JSON.
        #[arg(long, default_value = "\"boolean\"")]
        minimum: String,
        /// Stack after habit SID.
        #[arg(long)]
        stack_after: Option<u32>,
        /// Delay in seconds after stacked habit completes.
        #[arg(long, default_value = "0")]
        stack_delay: u32,
    },
    /// List habits.
    #[command(alias = "ls")]
    List {
        /// Include archived habits.
        #[arg(long)]
        all: bool,
    },
    /// Show habit details and status.
    Show {
        /// Habit SID.
        sid: u32,
    },
    /// Log a habit completion.
    Log {
        /// Habit SID.
        sid: u32,
        /// Amount (e.g. "10", "true", "15min"). Default: "true".
        #[arg(default_value = "true")]
        amount: String,
        /// Notes.
        #[arg(long)]
        notes: Option<String>,
        /// Mark as a slip (for break habits).
        #[arg(long)]
        slip: bool,
    },
    /// Skip a habit for a date without breaking the streak.
    Skip {
        /// Habit SID.
        sid: u32,
        /// Date to skip in ISO format. Defaults to today.
        #[arg(long)]
        date: Option<String>,
        /// Reason for skipping.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Freeze a habit for a date.
    Freeze {
        /// Habit SID.
        sid: u32,
        /// Date to freeze in ISO format. Defaults to today.
        #[arg(long)]
        date: Option<String>,
    },
    /// Log a habit for a past date.
    Backfill {
        /// Habit SID.
        sid: u32,
        /// Date in ISO `YYYY-MM-DD` format.
        date: String,
        /// Amount.
        #[arg(default_value = "true")]
        amount: String,
    },
    /// Show streak history for a habit.
    Streaks {
        /// Habit SID.
        sid: u32,
    },
    /// Modify a habit.
    #[command(alias = "mod")]
    Modify {
        /// Habit SID.
        sid: u32,
        /// New title.
        #[arg(long)]
        title: Option<String>,
        /// New identity statement.
        #[arg(long)]
        identity: Option<String>,
        /// New cue.
        #[arg(long)]
        cue: Option<String>,
        /// New response.
        #[arg(long)]
        response: Option<String>,
        /// New reward.
        #[arg(long)]
        reward: Option<String>,
        /// Stack after habit SID (0 to clear).
        #[arg(long)]
        stack_after: Option<u32>,
    },
    /// Archive a habit.
    Archive {
        /// Habit SID.
        sid: u32,
    },
    /// Show habit status overview.
    Status,
    /// Log a slip for a break-bad-habit (convenience for `log --slip`).
    Slip {
        /// Habit SID.
        sid: u32,
        /// Notes about the slip.
        #[arg(long)]
        notes: Option<String>,
    },
    /// Add or manage reminders for a habit.
    Remind {
        /// Habit SID.
        sid: u32,
        /// Time of day (HH:MM, e.g. "07:00"). Adds a reminder.
        #[arg(long)]
        at: Option<String>,
        /// Days this reminder applies to (comma-separated, e.g. "monday,wednesday").
        #[arg(long)]
        days: Option<String>,
        /// Clear all reminders for this habit.
        #[arg(long)]
        clear: bool,
        /// List current reminders.
        #[arg(long)]
        list: bool,
    },
}
