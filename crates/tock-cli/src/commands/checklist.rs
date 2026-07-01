//! `tock checklist` — manage a task's checklist items (sub-task checkboxes).

use clap::{Args, Subcommand};

/// Checklist management arguments.
#[derive(Debug, Args)]
pub struct ChecklistArgs {
    /// Checklist subcommand.
    #[command(subcommand)]
    pub command: ChecklistCommand,
}

/// Checklist subcommands. Items are addressed by their 1-based position.
#[derive(Debug, Subcommand)]
pub enum ChecklistCommand {
    /// Append a checklist item to a task.
    Add {
        /// Task SID.
        sid: u32,
        /// Item text.
        #[arg(trailing_var_arg = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// List a task's checklist items.
    #[command(alias = "ls")]
    List {
        /// Task SID.
        sid: u32,
    },
    /// Mark a checklist item done.
    Check {
        /// Task SID.
        sid: u32,
        /// 1-based item index.
        index: u32,
    },
    /// Mark a checklist item not done.
    Uncheck {
        /// Task SID.
        sid: u32,
        /// 1-based item index.
        index: u32,
    },
    /// Remove a checklist item.
    #[command(alias = "remove")]
    Rm {
        /// Task SID.
        sid: u32,
        /// 1-based item index.
        index: u32,
    },
    /// Move a checklist item to a new position.
    Reorder {
        /// Task SID.
        sid: u32,
        /// 1-based source index.
        from: u32,
        /// 1-based destination index.
        to: u32,
    },
}
