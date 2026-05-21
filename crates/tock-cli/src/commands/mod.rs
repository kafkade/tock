//! CLI subcommands.

pub mod add;
pub mod done;
pub mod list;
pub mod modify;
pub mod project;
pub mod tag;
pub mod time;
pub mod views;

use clap::Subcommand;

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Add a new task. Supports sigils: #tag, !H/M/L, due:YYYY-MM-DD.
    Add {
        /// Task description (may include sigils).
        #[arg(trailing_var_arg = true, num_args = 1..)]
        words: Vec<String>,
    },
    /// Modify an existing task.
    #[command(alias = "mod")]
    Modify {
        /// Task SID to modify.
        sid: u32,
        /// Fields and values to change (e.g. title:"New title" !M #newtag).
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Mark task(s) as done.
    Done {
        /// Task SID(s).
        #[arg(required = true, num_args = 1..)]
        sids: Vec<u32>,
    },
    /// Cancel task(s).
    Cancel {
        /// Task SID(s).
        #[arg(required = true, num_args = 1..)]
        sids: Vec<u32>,
    },
    /// Soft-delete task(s).
    Delete {
        /// Task SID(s).
        #[arg(required = true, num_args = 1..)]
        sids: Vec<u32>,
    },
    /// List tasks.
    #[command(alias = "ls")]
    List {
        /// Filter expression (e.g. status:pending, project:myproj).
        #[arg(trailing_var_arg = true)]
        filter: Vec<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show task details.
    Show {
        /// Task SID.
        sid: u32,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Project management.
    Project(project::ProjectArgs),
    /// Area management.
    Area(project::AreaArgs),
    /// Tag management.
    Tag(tag::TagArgs),
    /// Show a built-in view (inbox, today, upcoming, anytime, someday, logbook).
    View {
        /// View name.
        name: String,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Time tracking.
    Time(time::TimeArgs),
    /// List available views.
    Views,
    /// Generate shell completion scripts.
    Completions {
        /// Shell to generate completions for: bash, zsh, fish, elvish, powershell.
        shell: clap_complete::Shell,
    },
    /// Export data to a file.
    Export {
        /// Format (currently only 'json').
        format: String,
        /// Output file path (stdout if omitted).
        #[arg(long, short)]
        out: Option<std::path::PathBuf>,
    },
    /// Import data from a file.
    Import {
        /// Format (currently only 'json').
        format: String,
        /// Input file path.
        #[arg(long, short)]
        file: std::path::PathBuf,
    },
}
