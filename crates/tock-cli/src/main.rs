//! # tock CLI
//!
//! Command-line interface for tock — unified task, habit, time, and
//! focus engine. Phase 1 implements the task management commands.

mod commands;
mod display;
mod tracing_setup;

use std::path::PathBuf;
use std::process;

use clap::{CommandFactory, Parser};
use commands::Commands;
use display::{OutputFormat, format_task_detail, format_tasks};
use rusqlite::Connection;

/// tock — unified personal productivity engine.
#[derive(Debug, Parser)]
#[command(name = "tock", version, about)]
struct Cli {
    /// Path to the vault file.
    #[arg(long, env = "TOCK_VAULT", default_value = "tock.tockvault")]
    vault: PathBuf,

    /// Vault password (reads from `TOCK_PASSWORD` env var; prompts if absent).
    #[arg(long, env = "TOCK_PASSWORD", hide_env_values = true)]
    password: Option<String>,

    /// Log format: human (default) or json.
    #[arg(long, env = "TOCK_LOG_FORMAT", default_value = "human")]
    log_format: String,

    /// Output format: table (default), compact, json.
    #[arg(long, default_value = "table")]
    format: String,

    /// Subcommand to execute.
    #[command(subcommand)]
    command: Commands,
}

fn main() {
    let cli = Cli::parse();
    tracing_setup::init_tracing(cli.log_format == "json");

    if let Err(error) = run(&cli) {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    if let Commands::Completions { shell } = &cli.command {
        let mut cmd = Cli::command();
        clap_complete::generate(*shell, &mut cmd, "tock", &mut std::io::stdout());
        return Ok(());
    }

    let password = cli.password.as_deref().map_or(b"" as &[u8], str::as_bytes);

    let vault = if cli.vault.exists() {
        tock_storage::open(&cli.vault, password)?
    } else {
        tracing::info!("vault does not exist, initializing");
        tock_storage::init(&cli.vault, password)?
    };

    let conn = vault.connection();

    match &cli.command {
        Commands::Add { .. }
        | Commands::Modify { .. }
        | Commands::Done { .. }
        | Commands::Cancel { .. }
        | Commands::Delete { .. }
        | Commands::List { .. }
        | Commands::Show { .. } => run_task_cmd(conn, &cli.command, &cli.format),
        Commands::Project(args) => run_project_cmd(conn, &args.command),
        Commands::Area(args) => run_area_cmd(conn, &args.command),
        Commands::Tag(args) => run_tag_cmd(conn, &args.command),
        Commands::View { name, json } => run_view_cmd(conn, name, *json, &cli.format),
        Commands::Views => {
            let today_str = today_string();
            for view in commands::views::all_views(&today_str) {
                println!("  {:<12}  {}", view.name, view.description);
            }
            Ok(())
        }
        Commands::Export { format, out } => {
            if !format.eq_ignore_ascii_case("json") {
                eprintln!("unsupported format: {format}");
                return Ok(());
            }
            let json = tock_export::json::export_tasks(conn)?;
            match out {
                Some(path) => std::fs::write(path, &json)?,
                None => println!("{json}"),
            }
            Ok(())
        }
        Commands::Import { format, file } => {
            if !format.eq_ignore_ascii_case("json") {
                eprintln!("unsupported format: {format}");
                return Ok(());
            }
            let json = std::fs::read_to_string(file)?;
            let count = tock_import::json::import_tasks(conn, &json)?;
            println!("Imported {count} task(s)");
            Ok(())
        }
        Commands::Completions { .. } => unreachable!("completions handled before vault open"),
    }
}

fn run_task_cmd(
    conn: &Connection,
    cmd: &Commands,
    global_format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        Commands::Add { words } => {
            let new_task = commands::add::parse_add_input(words);
            let task = tock_storage::repo::task_repo::insert(conn, &new_task)?;
            println!("Created task #{} — {}", task.sid, task.title);
        }
        Commands::Modify { sid, args } => {
            let patch = commands::modify::parse_modify_args(args);
            let task = tock_storage::repo::task_repo::update(conn, *sid, &patch)?;
            println!("Modified task #{} — {}", task.sid, task.title);
        }
        Commands::Done { sids } => {
            for sid in sids {
                let task = tock_storage::repo::task_repo::set_status(
                    conn,
                    *sid,
                    commands::done::done_status(),
                )?;
                println!("Completed task #{} — {}", task.sid, task.title);
            }
        }
        Commands::Cancel { sids } => {
            for sid in sids {
                let task = tock_storage::repo::task_repo::set_status(
                    conn,
                    *sid,
                    commands::done::cancel_status(),
                )?;
                println!("Cancelled task #{} — {}", task.sid, task.title);
            }
        }
        Commands::Delete { sids } => {
            for sid in sids {
                tock_storage::repo::task_repo::soft_delete(conn, *sid)?;
                println!("Deleted task #{sid}");
            }
        }
        Commands::List { filter, json } => {
            let _filter = commands::list::parse_filter(filter);
            let tasks = tock_storage::repo::task_repo::list(conn, false)?;
            let format = selected_output_format(global_format, *json);
            let rendered = format_tasks(&tasks, format);
            print_task_listing(&rendered, tasks.len(), format);
        }
        Commands::Show { sid, json } => {
            if let Some(task) = tock_storage::repo::task_repo::get_by_sid(conn, *sid)? {
                let format = selected_output_format(global_format, *json);
                println!("{}", format_task_detail(&task, format));
            } else {
                eprintln!("task #{sid} not found");
            }
        }
        _ => {}
    }
    Ok(())
}

fn run_project_cmd(
    conn: &Connection,
    cmd: &commands::project::ProjectCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        commands::project::ProjectCommand::Add { name } => {
            let new_proj = tock_core::domain::project::NewProject {
                name: name.clone(),
                notes: None,
                area_id: None,
                deadline: None,
            };
            let proj = tock_storage::repo::project_repo::insert(conn, &new_proj)?;
            println!("Created project #{} — {}", proj.sid, proj.name);
        }
        commands::project::ProjectCommand::List { all } => {
            let projects = tock_storage::repo::project_repo::list(conn, *all)?;
            for project in &projects {
                println!(
                    "#{:<4}  {:<7}  {}",
                    project.sid,
                    project.status.as_str(),
                    project.name
                );
            }
            println!("\n{} project(s)", projects.len());
        }
        commands::project::ProjectCommand::Archive { sid } => {
            tock_storage::repo::project_repo::archive(conn, *sid)?;
            println!("Archived project #{sid}");
        }
    }
    Ok(())
}

fn run_area_cmd(
    conn: &Connection,
    cmd: &commands::project::AreaCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        commands::project::AreaCommand::Add { name } => {
            let new_area = tock_core::domain::area::NewArea {
                name: name.clone(),
                color: None,
            };
            let area = tock_storage::repo::area_repo::insert(conn, &new_area)?;
            println!("Created area — {}", area.name);
        }
        commands::project::AreaCommand::List { all } => {
            let areas = tock_storage::repo::area_repo::list(conn, *all)?;
            for area in &areas {
                println!("  {}", area.name);
            }
            println!("\n{} area(s)", areas.len());
        }
    }
    Ok(())
}

fn run_tag_cmd(
    conn: &Connection,
    cmd: &commands::tag::TagCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        commands::tag::TagCommand::List => {
            let tags = tock_storage::repo::tag_repo::list_all(conn)?;
            for tag in &tags {
                println!("  #{}", tag.name);
            }
            println!("\n{} tag(s)", tags.len());
        }
        commands::tag::TagCommand::Rename { old, new } => {
            tock_storage::repo::tag_repo::rename(conn, old, new)?;
            println!("Renamed #{old} → #{new}");
        }
    }
    Ok(())
}

fn run_view_cmd(
    conn: &Connection,
    name: &str,
    json: bool,
    global_format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let today_str = today_string();
    let views = commands::views::all_views(&today_str);
    let Some(view) = views.iter().find(|candidate| candidate.name == name) else {
        eprintln!(
            "unknown view '{name}'. Available: {}",
            views
                .iter()
                .map(|candidate| candidate.name)
                .collect::<Vec<_>>()
                .join(", ")
        );
        return Ok(());
    };

    let all_tasks = tock_storage::repo::task_repo::list(conn, false)?;
    let filtered: Vec<_> = all_tasks
        .into_iter()
        .filter(|task| tock_parse::filter::matches(&view.filter, &TaskFilterable(task)))
        .collect();

    let format = selected_output_format(global_format, json);
    let rendered = format_tasks(&filtered, format);
    if matches!(format, OutputFormat::Json) {
        println!("{rendered}");
    } else {
        println!("── {} ({}) ──", view.name, view.description);
        print_task_listing(&rendered, filtered.len(), format);
    }
    Ok(())
}

fn selected_output_format(global_format: &str, json: bool) -> OutputFormat {
    if json {
        OutputFormat::Json
    } else {
        OutputFormat::from_str(global_format)
    }
}

fn print_task_listing(rendered: &str, count: usize, format: OutputFormat) {
    if !rendered.is_empty() {
        println!("{rendered}");
    }
    if !matches!(format, OutputFormat::Json) {
        println!("\n{count} task(s)");
    }
}

fn today_string() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    )
}

/// Adapter: makes `Task` implement `tock_parse::filter::Filterable`.
struct TaskFilterable<'a>(&'a tock_core::domain::task::Task);

impl tock_parse::filter::Filterable for TaskFilterable<'_> {
    fn status(&self) -> &str {
        self.0.status.as_str()
    }

    fn tags(&self) -> &[String] {
        &self.0.tags
    }

    fn priority(&self) -> Option<char> {
        self.0
            .priority
            .as_ref()
            .map(tock_core::domain::task::Priority::as_char)
    }

    fn project_name(&self) -> Option<&str> {
        // Project name resolution requires a DB lookup; for now return None.
        // A future PR can enrich tasks with the project name on load.
        None
    }

    fn deadline(&self) -> Option<&str> {
        self.0.deadline.as_deref()
    }

    fn start_date(&self) -> Option<&str> {
        self.0.start_date.as_deref()
    }

    fn is_evening(&self) -> bool {
        self.0.evening
    }

    fn is_deleted(&self) -> bool {
        self.0.deleted_at.is_some()
    }
}
