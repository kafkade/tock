//! `CalDAV` sync CLI subcommands.

use clap::Subcommand;

/// `CalDAV` sync subcommands.
#[derive(Debug, clap::Args)]
pub struct CalDavArgs {
    /// `CalDAV` subcommand.
    #[command(subcommand)]
    pub cmd: CalDavCmd,
}

/// `CalDAV` operations.
#[derive(Debug, Subcommand)]
pub enum CalDavCmd {
    /// Set up a `CalDAV` collection for syncing.
    Setup {
        /// `CalDAV` server base URL (e.g. <https://cal.example.com/dav/>).
        #[arg(long)]
        url: String,
        /// Username for authentication.
        #[arg(long)]
        user: String,
        /// Collection URL (full path to the calendar collection).
        #[arg(long)]
        collection: String,
        /// Display name for this collection.
        #[arg(long)]
        name: Option<String>,
        /// Read password from stdin instead of prompting.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Sync with a configured `CalDAV` collection.
    Sync {
        /// Collection URL to sync (syncs all if omitted).
        #[arg(long)]
        collection: Option<String>,
        /// Dry run — show what would change without applying.
        #[arg(long)]
        dry_run: bool,
    },
    /// Show `CalDAV` sync status and configured collections.
    Status,
    /// Remove a `CalDAV` collection configuration and unlink all resources.
    Remove {
        /// Collection URL to remove.
        url: String,
    },
}
