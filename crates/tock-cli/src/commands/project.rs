//! `tock project` and `tock area` — organizational commands.

use clap::{Args, Subcommand};

/// Project management arguments.
#[derive(Debug, Args)]
pub struct ProjectArgs {
    /// Project subcommand.
    #[command(subcommand)]
    pub command: ProjectCommand,
}

/// Project subcommands.
#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// Create a new project.
    Add {
        /// Project name.
        name: String,
    },
    /// List projects.
    #[command(alias = "ls")]
    List {
        /// Include archived projects.
        #[arg(long)]
        all: bool,
    },
    /// Archive a project.
    Archive {
        /// Project SID.
        sid: u32,
    },
}

/// Area management arguments.
#[derive(Debug, Args)]
pub struct AreaArgs {
    /// Area subcommand.
    #[command(subcommand)]
    pub command: AreaCommand,
}

/// Area subcommands.
#[derive(Debug, Subcommand)]
pub enum AreaCommand {
    /// Create a new area.
    Add {
        /// Area name.
        name: String,
    },
    /// List areas.
    #[command(alias = "ls")]
    List {
        /// Include archived areas.
        #[arg(long)]
        all: bool,
    },
}
