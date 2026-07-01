//! CLI subcommands.

pub mod account;
pub mod add;
pub mod caldav;
pub mod checklist;
pub mod config;
pub mod context;
pub mod done;
pub mod focus;
pub mod habit;
pub mod hooks_cmd;
pub mod list;
pub mod modify;
pub mod project;
pub mod report;
pub mod sync_cmd;
pub mod tag;
pub mod time;
pub mod uda;
pub mod views;

use clap::Subcommand;

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Add a new task. Supports sigils: #tag, !H/M/L, due:YYYY-MM-DD.
    Add {
        /// Recurrence: daily, weekly, monthly, yearly, every-3d, every-2w.
        #[arg(long)]
        recur: Option<String>,
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
    /// Undo the last mutating command.
    Undo,
    /// Redo the last undone command.
    Redo,
    /// Add a dependency: task <sid> depends on <dep-sid>.
    Depend {
        /// Task SID.
        sid: u32,
        /// Dependency SID.
        on: u32,
    },
    /// Remove a dependency.
    Undepend {
        /// Task SID.
        sid: u32,
        /// Dependency SID.
        from: u32,
    },
    /// Manage a task's checklist items (sub-task checkboxes).
    #[command(alias = "cl")]
    Checklist(checklist::ChecklistArgs),
    /// Attach a timestamped annotation to a task.
    Annotate {
        /// Task SID.
        sid: u32,
        /// Annotation text.
        #[arg(trailing_var_arg = true, num_args = 1..)]
        words: Vec<String>,
    },
    /// Remove an annotation from a task by its 1-based index (see `tock show`).
    Denotate {
        /// Task SID.
        sid: u32,
        /// 1-based annotation index as shown by `tock show`.
        index: usize,
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
    /// Explain urgency score for a task.
    Urgency {
        /// Task SID.
        sid: u32,
    },
    /// Project management.
    Project(project::ProjectArgs),
    /// Area management.
    Area(project::AreaArgs),
    /// Tag management.
    Tag(tag::TagArgs),
    /// Saved custom reports.
    Report(report::ReportArgs),
    /// Named filter contexts.
    Context(context::ContextArgs),
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
    /// Focus timer.
    Focus(focus::FocusArgs),
    /// Habit tracking.
    Habit(habit::HabitArgs),
    /// Hook script management.
    Hooks(hooks_cmd::HooksArgs),
    /// User-defined attribute management.
    Uda(uda::UdaArgs),
    /// Inspect and scaffold the user configuration file.
    Config(config::ConfigArgs),
    /// List available views.
    Views,
    /// Launch the interactive terminal user interface.
    Tui,
    /// Generate shell completion scripts.
    Completions {
        /// Shell to generate completions for: bash, zsh, fish, elvish, powershell.
        shell: clap_complete::Shell,
    },
    /// `CalDAV` bidirectional sync.
    Caldav(caldav::CalDavArgs),
    /// Multi-device sync: push/pull events and review conflicts.
    Sync(sync_cmd::SyncArgs),
    /// Account signup, login, logout, and status (Secret Key + Emergency Kit).
    Account(account::AccountArgs),
    /// Pair a new device with an existing vault.
    Onboard(sync_cmd::OnboardArgs),
    /// Inspect and revoke registered devices.
    Device(sync_cmd::DeviceArgs),
    /// Export data to a file.
    Export {
        /// Format: 'json' or 'md'.
        format: String,
        /// Output file path (stdout if omitted).
        #[arg(long, short)]
        out: Option<std::path::PathBuf>,
        /// Built-in Markdown template: task-list, habit-report, time-report.
        #[arg(long)]
        builtin: Option<String>,
        /// Path to a custom Tera template file (Markdown export only).
        #[arg(long)]
        template: Option<std::path::PathBuf>,
        /// Task filter expressions (Markdown export only).
        #[arg(long)]
        filter: Vec<String>,
    },
    /// Import data from a file.
    Import {
        /// Format: 'json', 'taskwarrior', 'things3', or 'csv'.
        format: String,
        /// Input file path.
        #[arg(long, short)]
        file: std::path::PathBuf,
        /// Column mapping TOML file (CSV only).
        #[arg(long, short)]
        map: Option<std::path::PathBuf>,
        /// Include trashed items (Things 3 only).
        #[arg(long)]
        include_trash: bool,
    },
}
