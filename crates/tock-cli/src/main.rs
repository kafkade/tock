//! # tock CLI
//!
//! Command-line interface for tock — unified task, habit, time, and
//! focus engine. Phase 1 implements the task management commands.

mod commands;
mod display;
mod hooks;
mod http_transport;
mod notify;
mod tracing_setup;
mod tui;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process;

use clap::{CommandFactory, Parser};
use commands::{
    Commands, context::ContextCommand, focus::FocusCommand, habit::HabitCommand,
    hooks_cmd::HooksCommand, time::TimeCommand, uda::UdaCommand,
};
use display::{OutputFormat, format_task_detail, format_tasks};
use notify::notify;
use rusqlite::Connection;
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use tock_core::domain::cadence::ParsedCadence;
use tock_core::domain::task::{NewTask, Task, TaskStatus};
use uuid::Uuid;

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

#[derive(Clone, Debug)]
struct ActiveContext {
    name: String,
    filter: String,
}

#[derive(Clone, Copy, Debug, Default)]
struct TaskFilterState {
    is_blocked: bool,
    is_blocking: bool,
}

#[derive(Clone, Debug, Serialize)]
struct RelatedTaskSummary {
    sid: u32,
    title: String,
    status: String,
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
    #![allow(clippy::too_many_lines)]
    match &cli.command {
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(*shell, &mut cmd, "tock", &mut std::io::stdout());
            return Ok(());
        }
        Commands::Hooks(args) => {
            run_hooks_cmd(&args.command);
            return Ok(());
        }
        Commands::Onboard(args) => {
            return run_onboard_cmd(cli, &args.cmd);
        }
        _ => {}
    }

    let password = cli.password.as_deref().map_or(b"" as &[u8], str::as_bytes);

    let mut vault = if cli.vault.exists() {
        tock_storage::open(&cli.vault, password)?
    } else {
        tracing::info!("vault does not exist, initializing");
        tock_storage::init(&cli.vault, password)?
    };

    // Handle imports that need &mut Connection (for transactions) early.
    if let Commands::Import { format, file, map } = &cli.command
        && run_transactional_import(&mut vault, format, file, map.as_deref())?
    {
        return Ok(());
    }

    // Sync and device commands need the whole `OpenVault`, not just the
    // connection, so handle them before the connection borrow below.
    match &cli.command {
        Commands::Sync(args) => return commands::sync_cmd::run_sync(&vault, args),
        Commands::Device(args) => return commands::sync_cmd::run_device(&vault, &args.cmd),
        _ => {}
    }

    let conn = vault.connection();
    let active_context = load_active_context(conn)?;

    match &cli.command {
        Commands::Add { .. }
        | Commands::Modify { .. }
        | Commands::Done { .. }
        | Commands::Cancel { .. }
        | Commands::Delete { .. }
        | Commands::Depend { .. }
        | Commands::Undepend { .. }
        | Commands::List { .. }
        | Commands::Show { .. }
        | Commands::Urgency { .. } => {
            run_task_cmd(conn, &cli.command, &cli.format, active_context.as_ref())
        }
        Commands::Project(args) => run_project_cmd(conn, &args.command),
        Commands::Area(args) => run_area_cmd(conn, &args.command),
        Commands::Tag(args) => run_tag_cmd(conn, &args.command),
        Commands::Report(args) => run_report_cmd(conn, &args.command, active_context.as_ref()),
        Commands::Context(args) => run_context_cmd(conn, &args.command),
        Commands::Time(args) => run_time_cmd(conn, &args.command),
        Commands::Focus(args) => run_focus_cmd(conn, &args.command),
        Commands::Habit(args) => run_habit_cmd(conn, &args.command),
        Commands::Uda(args) => run_uda_cmd(conn, &args.command),
        Commands::Tui => {
            tui::run(conn)?;
            Ok(())
        }
        Commands::View { name, json } => {
            run_view_cmd(conn, name, *json, &cli.format, active_context.as_ref())
        }
        Commands::Views => {
            let today_str = today_string();
            for view in commands::views::all_views(&today_str) {
                println!("  {:<12}  {}", view.name, view.description);
            }
            Ok(())
        }
        Commands::Caldav(args) => run_caldav_cmd(conn, args),
        Commands::Export {
            format,
            out,
            builtin,
            template,
            filter,
        } => run_export_cmd(
            conn,
            format,
            out.as_deref(),
            builtin.as_deref(),
            template.as_deref(),
            filter,
        ),
        Commands::Import { format, file, .. } => {
            if format.eq_ignore_ascii_case("json") {
                let contents = std::fs::read_to_string(file)?;
                let count = tock_import::json::import_tasks(conn, &contents)?;
                println!("Imported {count} task(s)");
            } else {
                eprintln!("unsupported format: {format} (supported: json, taskwarrior, csv)");
            }
            Ok(())
        }
        Commands::Completions { .. } => unreachable!("completions handled before vault open"),
        Commands::Hooks(_) => unreachable!("hooks handled before vault open"),
        Commands::Onboard(_) => unreachable!("onboard handled before vault open"),
        Commands::Sync(_) | Commands::Device(_) => {
            unreachable!("sync/device handled before connection borrow")
        }
    }
}

/// Handle `tock onboard` subcommands. Invite opens the existing vault;
/// Accept creates a fresh vault, so this runs before the normal
/// open-or-init path in [`run`].
fn run_onboard_cmd(
    cli: &Cli,
    cmd: &commands::sync_cmd::OnboardCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    use commands::sync_cmd::OnboardCmd;

    let password = cli.password.as_deref().map_or(b"" as &[u8], str::as_bytes);
    match cmd {
        OnboardCmd::Invite { server } => {
            if !cli.vault.exists() {
                return Err("vault does not exist; nothing to invite from".into());
            }
            let vault = tock_storage::open(&cli.vault, password)?;
            commands::sync_cmd::run_onboard_invite(&vault, server.as_deref())
        }
        OnboardCmd::Accept {
            server,
            vault_id,
            inviter_pubkey,
            inviter_fingerprint,
        } => commands::sync_cmd::run_onboard_accept(
            &cli.vault,
            password,
            server,
            vault_id,
            inviter_pubkey,
            inviter_fingerprint,
        ),
    }
}

/// Handle import formats that require `&mut Connection` (for transactions).
/// Returns `true` if the import was handled, `false` if the format needs
/// the normal (immutable) code path.
fn run_transactional_import(
    vault: &mut tock_storage::OpenVault,
    format: &str,
    file: &std::path::Path,
    map: Option<&std::path::Path>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if format.eq_ignore_ascii_case("taskwarrior") {
        let contents = std::fs::read_to_string(file)?;
        let conn_mut = vault.connection_mut();
        let report = tock_import::taskwarrior::import_taskwarrior(conn_mut, &contents)?;
        print!("{report}");
        return Ok(true);
    }
    if format.eq_ignore_ascii_case("csv") {
        let contents = std::fs::read_to_string(file)?;
        let mapping_toml = map.map(std::fs::read_to_string).transpose()?;
        let conn_mut = vault.connection_mut();
        let report =
            tock_import::csv_import::import_csv(conn_mut, &contents, mapping_toml.as_deref())?;
        print!("{report}");
        return Ok(true);
    }
    Ok(false)
}

/// Handle `tock caldav` subcommands.
fn run_caldav_cmd(
    conn: &Connection,
    args: &commands::caldav::CalDavArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    use commands::caldav::CalDavCmd;
    use tock_storage::repo::caldav_link_repo;

    match &args.cmd {
        CalDavCmd::Setup {
            url: _,
            user,
            collection,
            name,
            password_stdin: _,
        } => caldav_setup(conn, user, collection, name.as_deref()),
        CalDavCmd::Sync {
            collection,
            dry_run,
        } => caldav_sync(conn, collection.as_deref(), *dry_run),
        CalDavCmd::Status => caldav_status(conn),
        CalDavCmd::Remove { url } => {
            caldav_link_repo::delete_collection(conn, url)?;
            println!("Removed CalDAV collection: {url}");
            println!("All links to this collection have been deleted.");
            Ok(())
        }
    }
}

/// Register a `CalDAV` collection.
fn caldav_setup(
    conn: &Connection,
    user: &str,
    collection: &str,
    name: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tock_storage::repo::caldav_link_repo;

    let row = caldav_link_repo::CalDavCollectionRow {
        url: collection.into(),
        display_name: name.map(String::from),
        sync_token: None,
        ctag: None,
        username: user.into(),
        last_sync_at: None,
    };
    caldav_link_repo::upsert_collection(conn, &row)?;
    println!(
        "CalDAV collection configured: {}",
        name.unwrap_or(collection)
    );
    println!("  URL: {collection}");
    println!("  User: {user}");
    println!("\nRun `tock caldav sync` to start syncing.");
    Ok(())
}

/// Run `CalDAV` sync for one or all collections.
#[allow(clippy::too_many_lines)]
fn caldav_sync(
    conn: &Connection,
    collection: Option<&str>,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use tock_storage::repo::caldav_link_repo;

    let collections = if let Some(url) = collection {
        let Some(c) = caldav_link_repo::get_collection(conn, url)? else {
            eprintln!("No collection configured at: {url}");
            eprintln!("Run `tock caldav setup` first.");
            return Ok(());
        };
        vec![c]
    } else {
        caldav_link_repo::list_collections(conn)?
    };

    if collections.is_empty() {
        eprintln!("No CalDAV collections configured.");
        eprintln!("Run `tock caldav setup` to add one.");
        return Ok(());
    }

    for col in &collections {
        let name = col.display_name.as_deref().unwrap_or(&col.url);
        println!("Syncing collection: {name}");

        let tasks = tock_storage::repo::task_repo::list(conn, false)?;
        let time_blocks = tock_storage::repo::time_block_repo::list(conn, true)?;
        let links = caldav_link_repo::list_links(conn, &col.url)?;

        let sync_links: Vec<tock_caldav::sync::CalDavLink> = links
            .iter()
            .filter_map(|l| {
                tock_caldav::sync::EntityType::from_str_opt(&l.entity_type).map(|et| {
                    tock_caldav::sync::CalDavLink {
                        local_id: l.local_id,
                        entity_type: et,
                        collection_url: l.collection_url.clone(),
                        href: l.href.clone(),
                        uid: l.uid.clone(),
                        etag: l.etag.clone(),
                        last_pushed_at: l.last_pushed_at.clone(),
                        last_pulled_at: l.last_pulled_at.clone(),
                    }
                })
            })
            .collect();

        let push_actions =
            tock_caldav::sync::compute_push_actions(&tasks, &time_blocks, &sync_links, &col.url);

        if dry_run {
            println!("  Would push {} resource(s)", push_actions.len());
            for action in &push_actions {
                if let tock_caldav::sync::SyncAction::Push {
                    entity_type,
                    href,
                    etag,
                    ..
                } = action
                {
                    let op = if etag.is_some() { "update" } else { "create" };
                    println!("    {op} {} → {href}", entity_type.as_str());
                }
            }
            println!("  (dry run — no changes applied)");
        } else {
            println!(
                "  {} task(s), {} time block(s), {} existing link(s)",
                tasks.len(),
                time_blocks.len(),
                sync_links.len()
            );
            println!("  {} resource(s) to push", push_actions.len());
            eprintln!("  Note: CalDAV sync requires a network transport implementation.");
            eprintln!("  The sync engine is ready but HTTP transport is not yet wired.");
        }
    }
    Ok(())
}

/// Show `CalDAV` sync status.
fn caldav_status(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    use tock_storage::repo::caldav_link_repo;

    let collections = caldav_link_repo::list_collections(conn)?;
    if collections.is_empty() {
        println!("No CalDAV collections configured.");
        println!("Run `tock caldav setup` to add one.");
        return Ok(());
    }
    println!("CalDAV collections:\n");
    for col in &collections {
        let name = col.display_name.as_deref().unwrap_or("(unnamed)");
        println!("  {name}");
        println!("    URL:        {}", col.url);
        println!("    User:       {}", col.username);
        if let Some(ref token) = col.sync_token {
            println!("    Sync token: {token}");
        }
        if let Some(ref last) = col.last_sync_at {
            println!("    Last sync:  {last}");
        } else {
            println!("    Last sync:  never");
        }
        let links = caldav_link_repo::list_links(conn, &col.url)?;
        println!("    Linked:     {} resource(s)", links.len());
        println!();
    }
    Ok(())
}

/// Handle `tock export md` — render Markdown via Tera templates.
/// Dispatch `tock export <format>`.
fn run_export_cmd(
    conn: &Connection,
    format: &str,
    out: Option<&std::path::Path>,
    builtin: Option<&str>,
    template_path: Option<&std::path::Path>,
    filter: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    if format.eq_ignore_ascii_case("json") {
        let json = tock_export::json::export_tasks(conn)?;
        match out {
            Some(path) => std::fs::write(path, &json)?,
            None => println!("{json}"),
        }
    } else if format.eq_ignore_ascii_case("md") || format.eq_ignore_ascii_case("markdown") {
        run_export_md(conn, out, builtin, template_path, filter)?;
    } else {
        eprintln!("unsupported format: {format} (supported: json, md)");
    }
    Ok(())
}

/// Handle `tock export md` — render Markdown via Tera templates.
fn run_export_md(
    conn: &Connection,
    out: Option<&std::path::Path>,
    builtin: Option<&str>,
    template_path: Option<&std::path::Path>,
    filter: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    use tock_export::markdown::{BuiltinTemplate, TemplateSource, build_context, render_markdown};

    // Load domain data.
    let mut tasks = tock_storage::repo::task_repo::list(conn, false)?;
    let habits = tock_storage::repo::habit_repo::list(conn, false)?;
    let time_blocks = tock_storage::repo::time_block_repo::list(conn, true)?;
    let projects = tock_storage::repo::project_repo::list(conn, false)?;

    // Apply task filters if provided.
    if !filter.is_empty() {
        let list_filter = commands::list::parse_filter(filter);
        tasks = apply_list_filter(tasks, &list_filter, &projects);
    }

    // Determine template source.
    let custom_content;
    let template_source = if let Some(path) = template_path {
        custom_content = std::fs::read_to_string(path)?;
        TemplateSource::Custom(&custom_content)
    } else {
        let bt = builtin
            .map(BuiltinTemplate::parse_name)
            .transpose()
            .map_err(|e| format!("{e}"))?
            .unwrap_or(BuiltinTemplate::TaskList);
        TemplateSource::Builtin(bt)
    };

    let context = build_context(&tasks, &habits, &time_blocks, &projects);
    let md = render_markdown(&context, &template_source).map_err(|e| format!("{e}"))?;

    match out {
        Some(path) => std::fs::write(path, &md)?,
        None => print!("{md}"),
    }
    Ok(())
}

/// Apply a basic list filter to tasks, matching the existing list command logic.
fn apply_list_filter(
    tasks: Vec<Task>,
    filter: &commands::list::ListFilter,
    projects: &[tock_core::domain::project::Project],
) -> Vec<Task> {
    let project_map: HashMap<uuid::Uuid, String> =
        projects.iter().map(|p| (p.id, p.name.clone())).collect();

    tasks
        .into_iter()
        .filter(|t| {
            if let Some(ref status) = filter.status
                && t.status.as_str() != status
            {
                return false;
            }
            if let Some(ref project) = filter.project {
                let name = t
                    .project_id
                    .and_then(|id| project_map.get(&id))
                    .map_or("", String::as_str);
                if !name.eq_ignore_ascii_case(project) {
                    return false;
                }
            }
            if let Some(ref tag) = filter.tag
                && !t.tags.iter().any(|tg| tg.eq_ignore_ascii_case(tag))
            {
                return false;
            }
            if let Some(ref priority) = filter.priority {
                let p_str = t
                    .priority
                    .as_ref()
                    .map(|p| p.as_char().to_string())
                    .unwrap_or_default();
                if !p_str.eq_ignore_ascii_case(priority) {
                    return false;
                }
            }
            true
        })
        .collect()
}

fn run_task_cmd(
    conn: &Connection,
    cmd: &Commands,
    global_format: &str,
    active_context: Option<&ActiveContext>,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        Commands::Add { words, recur, .. } => run_add_cmd(conn, words, recur.as_deref())?,
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
                // Auto-stop any active focus session linked to this task.
                if let Some(active) = tock_storage::repo::focus_repo::get_active(conn)?
                    && active.task_id == Some(task.id)
                {
                    let _ = tock_storage::repo::focus_repo::abort(conn, active.sid);
                    println!("  (auto-stopped focus session #{})", active.sid);
                }
                // Auto-stop any running time block linked to this task.
                if let Some(running) = tock_storage::repo::time_block_repo::get_current(conn)?
                    && running.task_id == Some(task.id)
                {
                    let _ = tock_storage::repo::time_block_repo::stop(conn, running.sid);
                    println!("  (auto-stopped timer #{})", running.sid);
                }
                println!("Completed task #{} — {}", task.sid, task.title);
                let _ = hooks::run_hook(hooks::HookEvent::OnComplete, &task_to_hook_json(&task));
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
        Commands::Depend { sid, on } => {
            tock_storage::repo::task_repo::add_dependency(conn, *sid, *on)?;
            println!("Task #{sid} now depends on #{on}");
        }
        Commands::Undepend { sid, from } => {
            tock_storage::repo::task_repo::remove_dependency(conn, *sid, *from)?;
            println!("Task #{sid} no longer depends on #{from}");
        }
        Commands::List { filter, json } => {
            let today = today_string();
            let filter_args = filter.iter().map(String::as_str).collect::<Vec<_>>();
            let base_filter = tock_parse::filter::parse_filter(&filter_args, &today);
            let parsed_filter = combine_with_active_context(base_filter, active_context, &today);
            let tasks = tock_storage::repo::task_repo::list(conn, false)?;
            let filtered = filter_tasks(conn, tasks, &parsed_filter)?;
            let format = selected_output_format(global_format, *json);
            let rendered = format_tasks(&filtered, format);
            print_task_listing(
                &rendered,
                filtered.len(),
                format,
                active_context.map(|ctx| ctx.name.as_str()),
            );
        }
        Commands::Show { sid, json } => {
            if let Some(task) = tock_storage::repo::task_repo::get_by_sid(conn, *sid)? {
                let format = selected_output_format(global_format, *json);
                print_task_show(conn, &task, format)?;
            } else {
                eprintln!("task #{sid} not found");
            }
        }
        Commands::Urgency { sid } => run_urgency_cmd(conn, *sid)?,
        _ => {}
    }
    Ok(())
}

fn run_add_cmd(
    conn: &Connection,
    words: &[String],
    recur: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut parsed_task = commands::add::parse_add_input(words);
    parsed_task.recurrence =
        commands::add::parse_recur_flag(recur).map_err(std::io::Error::other)?;
    let hook_input = new_task_to_hook_json(&parsed_task);
    let Some(hooked_json) = hooks::run_hook(hooks::HookEvent::OnAdd, &hook_input) else {
        println!("Add cancelled by hook");
        return Ok(());
    };
    let new_task = new_task_from_hook_json(&parsed_task, &hooked_json)?;
    let task = tock_storage::repo::task_repo::insert(conn, &new_task)?;
    println!("Created task #{} — {}", task.sid, task.title);
    Ok(())
}

fn run_urgency_cmd(conn: &Connection, sid: u32) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(task) = tock_storage::repo::task_repo::get_by_sid(conn, sid)? {
        let breakdown = explain_task_urgency(
            &task,
            tock_storage::repo::task_repo::is_blocked(conn, task.id)?,
        );
        let total: f64 = breakdown
            .iter()
            .map(|(_, _, _, contribution)| contribution)
            .sum();
        println!("Urgency for task #{} — {}", task.sid, task.title);
        for (component, weight, factor, contribution) in breakdown {
            println!(
                "  {component:<12} weight={weight:>6.2} factor={factor:>6.2} contribution={contribution:>7.2}"
            );
        }
        println!("  {:<12} {:>30.2}", "total", total);
    } else {
        eprintln!("task #{sid} not found");
    }
    Ok(())
}

fn run_context_cmd(
    conn: &Connection,
    cmd: &ContextCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ContextCommand::Set { name } => {
            tock_storage::repo::context_repo::set_active(conn, Some(name))?;
            println!("Activated context {name}");
        }
        ContextCommand::Clear => {
            tock_storage::repo::context_repo::set_active(conn, None)?;
            println!("Cleared active context");
        }
        ContextCommand::Define { name, filter } => {
            tock_storage::repo::context_repo::define(conn, name, filter)?;
            println!("Defined context {name}");
        }
        ContextCommand::List => {
            let active = tock_storage::repo::context_repo::get_active(conn)?;
            let contexts = tock_storage::repo::context_repo::list(conn)?;
            if contexts.is_empty() {
                println!("No contexts defined");
            } else {
                for (name, filter) in contexts {
                    let marker = if active.as_deref() == Some(name.as_str()) {
                        '*'
                    } else {
                        ' '
                    };
                    println!("{marker} {name:<16} {filter}");
                }
            }
        }
        ContextCommand::Rm { name } => {
            tock_storage::repo::context_repo::delete(conn, name)?;
            println!("Removed context {name}");
        }
    }
    Ok(())
}

fn run_hooks_cmd(cmd: &HooksCommand) {
    match cmd {
        HooksCommand::List => {
            let installed = hooks::list_hooks();
            if installed.is_empty() {
                println!("No hooks installed");
            } else {
                for (event, path) in installed {
                    println!("{:<16} {}", event.script_name(), path.display());
                }
            }
        }
        HooksCommand::Path => println!("{}", hooks::hooks_dir().display()),
    }
}

fn run_habit_cmd(conn: &Connection, cmd: &HabitCommand) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        HabitCommand::Add {
            title,
            identity,
            cue,
            response,
            reward,
            direction,
            cadence,
            minimum,
            stack_after,
            stack_delay,
        } => run_habit_add(
            conn,
            title,
            identity.as_deref(),
            cue.as_deref(),
            response.as_deref(),
            reward.as_deref(),
            direction,
            cadence,
            minimum,
            *stack_after,
            *stack_delay,
        ),
        HabitCommand::List { all } => run_habit_list(conn, *all),
        HabitCommand::Show { sid } => run_habit_show(conn, *sid),
        HabitCommand::Log {
            sid,
            amount,
            notes,
            slip,
        } => run_habit_log(conn, *sid, amount, notes.as_deref(), *slip),
        HabitCommand::Skip { sid, date, reason } => {
            run_habit_skip(conn, *sid, date.as_deref(), reason.as_deref())
        }
        HabitCommand::Freeze { sid, date } => run_habit_freeze(conn, *sid, date.as_deref()),
        HabitCommand::Backfill { sid, date, amount } => {
            run_habit_backfill(conn, *sid, date, amount)
        }
        HabitCommand::Streaks { sid } => run_habit_streaks(conn, *sid),
        HabitCommand::Modify {
            sid,
            title,
            identity,
            cue,
            response,
            reward,
            stack_after,
        } => run_habit_modify(
            conn,
            *sid,
            title.as_deref(),
            identity.as_deref(),
            cue.as_deref(),
            response.as_deref(),
            reward.as_deref(),
            *stack_after,
        ),
        HabitCommand::Archive { sid } => run_habit_archive(conn, *sid),
        HabitCommand::Status => run_habit_status(conn),
        HabitCommand::Slip { sid, notes } => {
            let entry = tock_storage::repo::habit_repo::log_entry(
                conn,
                *sid,
                "true",
                notes.as_deref(),
                true,
            )?;
            let habit =
                tock_storage::repo::habit_repo::get_by_sid(conn, *sid)?.ok_or("habit not found")?;
            println!(
                "🚫 Slip logged for #{} — {} ({} {})",
                habit.sid,
                habit.title,
                habit.streak_current,
                habit.streak_label()
            );
            let _ = entry;
            Ok(())
        }
        HabitCommand::Remind {
            sid,
            at,
            days,
            clear,
            list,
        } => run_habit_remind(conn, *sid, at.as_deref(), days.as_deref(), *clear, *list),
    }
}

fn run_habit_remind(
    conn: &Connection,
    sid: u32,
    at: Option<&str>,
    days: Option<&str>,
    clear: bool,
    list: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if clear {
        tock_storage::repo::habit_repo::set_reminders(conn, sid, &[])?;
        println!("Cleared all reminders for habit #{sid}");
        return Ok(());
    }
    if list {
        let reminders = tock_storage::repo::habit_repo::get_reminders(conn, sid)?;
        if reminders.is_empty() {
            println!("No reminders set for habit #{sid}");
        } else {
            for r in &reminders {
                println!("  🔔 {}", r.display());
            }
        }
        return Ok(());
    }
    if let Some(time) = at {
        let mut reminders = tock_storage::repo::habit_repo::get_reminders(conn, sid)?;
        let day_list = days
            .map(|d| d.split(',').map(|s| s.trim().to_lowercase()).collect())
            .unwrap_or_default();
        reminders.push(tock_core::domain::habit::Reminder {
            time: time.to_string(),
            days: day_list,
        });
        tock_storage::repo::habit_repo::set_reminders(conn, sid, &reminders)?;
        println!("Added reminder at {time} for habit #{sid}");
    } else {
        eprintln!("Specify --at <HH:MM>, --list, or --clear");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_habit_add(
    conn: &Connection,
    title: &str,
    identity: Option<&str>,
    cue: Option<&str>,
    response: Option<&str>,
    reward: Option<&str>,
    direction: &str,
    cadence: &str,
    minimum: &str,
    stack_after: Option<u32>,
    stack_delay: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let new_habit = tock_core::domain::habit::NewHabit {
        title: title.to_owned(),
        identity: identity.map(str::to_owned),
        cue: cue.map(str::to_owned),
        craving: None,
        response: response.map(str::to_owned),
        reward: reward.map(str::to_owned),
        direction: parse_habit_direction_arg(direction)?,
        cadence: cadence.to_owned(),
        minimum: minimum.to_owned(),
        stack_after,
        stack_delay_s: stack_delay,
        area_id: None,
        project_id: None,
    };
    let habit = tock_storage::repo::habit_repo::insert(conn, &new_habit)?;
    println!("Created habit #{} — {}", habit.sid, habit.title);
    Ok(())
}

fn run_habit_list(conn: &Connection, all: bool) -> Result<(), Box<dyn std::error::Error>> {
    let habits = tock_storage::repo::habit_repo::list(conn, all)?;
    println!(
        "{:>4}  {:<7}  {:<12}  {:>8}  {:<28}  Identity",
        "SID", "Dir", "Level", "Streak", "Title"
    );
    for habit in &habits {
        println!(
            "{:>4}  {:<7}  {:<12}  {:>8}  {:<28}  {}",
            habit.sid,
            habit.direction.as_str(),
            format!("L{} ({})", habit.level, habit.level_name()),
            format!("{}d", habit.streak_current),
            truncate_str(&habit.title, 28),
            truncate_str(habit.identity.as_deref().unwrap_or("—"), 40)
        );
    }
    println!("\n{} habit(s)", habits.len());
    Ok(())
}

fn run_habit_show(conn: &Connection, sid: u32) -> Result<(), Box<dyn std::error::Error>> {
    let Some(habit) = tock_storage::repo::habit_repo::get_by_sid(conn, sid)? else {
        eprintln!("habit #{sid} not found");
        return Ok(());
    };
    let all_habits = tock_storage::repo::habit_repo::list(conn, true)?;
    let sid_by_id: std::collections::HashMap<_, _> = all_habits
        .iter()
        .map(|candidate| (candidate.id, candidate.sid))
        .collect();
    let entries = tock_storage::repo::habit_repo::get_entries(conn, habit.sid)?;
    let stacked = tock_storage::repo::habit_repo::get_stacked_habits(conn, habit.id)?;
    let parent = habit.stack_after.and_then(|parent_id| {
        all_habits
            .iter()
            .find(|candidate| candidate.id == parent_id)
            .map(|candidate| format!("#{} — {}", candidate.sid, candidate.title))
    });
    let archived = match habit.archived_at {
        Some(timestamp) => format_timestamp_full(timestamp)?,
        None => String::from("—"),
    };

    println!("#{} — {}", habit.sid, habit.title);
    println!("Direction: {}", habit.direction.as_str());
    println!("Identity: {}", habit.identity.as_deref().unwrap_or("—"));
    println!("Cue: {}", habit.cue.as_deref().unwrap_or("—"));
    println!("Craving: {}", habit.craving.as_deref().unwrap_or("—"));
    println!("Response: {}", habit.response.as_deref().unwrap_or("—"));
    println!("Reward: {}", habit.reward.as_deref().unwrap_or("—"));
    println!("Cadence: {}", habit_cadence_display(&habit.cadence));
    println!("Minimum: {}", habit.minimum);
    println!("Level: L{} ({})", habit.level, habit.level_name());
    println!("XP: {}", format_habit_xp(habit.level, habit.xp));
    println!(
        "Streaks: current {}d / best {}d",
        habit.streak_current, habit.streak_best
    );
    println!(
        "Stacking: {}",
        parent.map_or_else(
            || String::from("none"),
            |label| {
                if habit.stack_delay_s == 0 {
                    label
                } else {
                    format!("{label} + {}", format_stack_delay(habit.stack_delay_s))
                }
            }
        )
    );
    println!(
        "Area: {}",
        habit
            .area_id
            .map_or_else(|| String::from("—"), |id| id.to_string())
    );
    println!(
        "Project: {}",
        habit
            .project_id
            .map_or_else(|| String::from("—"), |id| id.to_string())
    );
    println!("Created: {}", format_timestamp_full(habit.created_at)?);
    println!("Modified: {}", format_timestamp_full(habit.modified_at)?);
    println!("Archived: {archived}");
    println!(
        "Entries: {} (next: {})",
        entries.len(),
        next_due_text(&habit, &entries, &sid_by_id)
    );
    if stacked.is_empty() {
        println!("Stacked children: none");
    } else {
        println!("Stacked children:");
        for child in &stacked {
            if child.stack_delay_s == 0 {
                println!("  #{} — {}", child.sid, child.title);
            } else {
                println!(
                    "  #{} — {} (+{})",
                    child.sid,
                    child.title,
                    format_stack_delay(child.stack_delay_s)
                );
            }
        }
    }
    Ok(())
}

fn run_habit_log(
    conn: &Connection,
    sid: u32,
    amount: &str,
    notes: Option<&str>,
    slip: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let entry = tock_storage::repo::habit_repo::log_entry(conn, sid, amount, notes, slip)?;
    let habit = tock_storage::repo::habit_repo::get_by_sid(conn, sid)?
        .ok_or_else(|| std::io::Error::other(format!("habit #{sid} not found after logging")))?;
    let outcome = if entry.slip {
        "Logged slip"
    } else {
        "Logged habit"
    };
    println!(
        "{} #{} — streak {}d, {}",
        outcome,
        habit.sid,
        habit.streak_current,
        format_habit_xp(habit.level, habit.xp)
    );
    Ok(())
}

fn run_habit_skip(
    conn: &Connection,
    sid: u32,
    date: Option<&str>,
    reason: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let skip_date = date.map_or_else(today_string, str::to_owned);
    tock_storage::repo::habit_repo::add_skip(conn, sid, &skip_date, "skip", reason)?;
    let habit = tock_storage::repo::habit_repo::get_by_sid(conn, sid)?
        .ok_or_else(|| std::io::Error::other(format!("habit #{sid} not found after skip")))?;
    println!(
        "Skipped habit #{} on {} — streak {}d",
        habit.sid, skip_date, habit.streak_current
    );
    Ok(())
}

fn run_habit_freeze(
    conn: &Connection,
    sid: u32,
    date: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let freeze_date = date.map_or_else(today_string, str::to_owned);
    tock_storage::repo::habit_repo::add_skip(conn, sid, &freeze_date, "freeze", None)?;
    let habit = tock_storage::repo::habit_repo::get_by_sid(conn, sid)?
        .ok_or_else(|| std::io::Error::other(format!("habit #{sid} not found after freeze")))?;
    println!(
        "Froze habit #{} on {} — streak {}d",
        habit.sid, freeze_date, habit.streak_current
    );
    Ok(())
}

fn run_habit_backfill(
    conn: &Connection,
    sid: u32,
    date: &str,
    amount: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = tock_storage::repo::habit_repo::log_backfill(conn, sid, date, amount, None)?;
    let habit = tock_storage::repo::habit_repo::get_by_sid(conn, sid)?
        .ok_or_else(|| std::io::Error::other(format!("habit #{sid} not found after backfill")))?;
    println!(
        "Backfilled habit #{} on {} — streak {}d, {}",
        habit.sid,
        date,
        habit.streak_current,
        format_habit_xp(habit.level, habit.xp)
    );
    Ok(())
}

fn run_habit_streaks(conn: &Connection, sid: u32) -> Result<(), Box<dyn std::error::Error>> {
    let Some(habit) = tock_storage::repo::habit_repo::get_by_sid(conn, sid)? else {
        eprintln!("habit #{sid} not found");
        return Ok(());
    };
    let entries = tock_storage::repo::habit_repo::get_entries(conn, sid)?;

    println!("#{} — {}", habit.sid, habit.title);
    println!("Current streak: {}d", habit.streak_current);
    println!("Best streak: {}d", habit.streak_best);
    if entries.is_empty() {
        println!("Recent entries: none");
        return Ok(());
    }

    println!("Recent entries:");
    for entry in entries.iter().take(10) {
        let date = entry.occurred_at.date();
        println!(
            "  {:04}-{:02}-{:02}{}",
            date.year(),
            u8::from(date.month()),
            date.day(),
            if entry.slip { " (slip)" } else { "" }
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_habit_modify(
    conn: &Connection,
    sid: u32,
    title: Option<&str>,
    identity: Option<&str>,
    cue: Option<&str>,
    response: Option<&str>,
    reward: Option<&str>,
    stack_after: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let patch = tock_core::domain::habit::HabitPatch {
        title: title.map(str::to_owned),
        identity: match identity {
            Some(text) if text.trim().is_empty() => Some(None),
            Some(text) => Some(Some(text.to_owned())),
            None => None,
        },
        cue: match cue {
            Some(text) if text.trim().is_empty() => Some(None),
            Some(text) => Some(Some(text.to_owned())),
            None => None,
        },
        craving: None,
        response: match response {
            Some(text) if text.trim().is_empty() => Some(None),
            Some(text) => Some(Some(text.to_owned())),
            None => None,
        },
        reward: match reward {
            Some(text) if text.trim().is_empty() => Some(None),
            Some(text) => Some(Some(text.to_owned())),
            None => None,
        },
        stack_after: stack_after.map(|value| if value == 0 { None } else { Some(value) }),
        stack_delay_s: None,
    };
    let habit = tock_storage::repo::habit_repo::update(conn, sid, &patch)?;
    println!("Modified habit #{} — {}", habit.sid, habit.title);
    Ok(())
}

fn run_habit_archive(conn: &Connection, sid: u32) -> Result<(), Box<dyn std::error::Error>> {
    tock_storage::repo::habit_repo::archive(conn, sid)?;
    println!("Archived habit #{sid}");
    Ok(())
}

fn run_habit_status(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let habits = tock_storage::repo::habit_repo::list(conn, false)?;
    if habits.is_empty() {
        println!("No active habits");
        return Ok(());
    }
    for habit in &habits {
        println!("{}", format_habit_status_line(habit));
    }
    println!("\n{} habit(s)", habits.len());
    Ok(())
}

fn run_focus_cmd(
    conn: &Connection,
    cmd: &commands::focus::FocusCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        FocusCommand::Start {
            task,
            cycles,
            work,
            short_break,
            long_break,
        } => run_focus_start(conn, *task, *cycles, *work, *short_break, *long_break),
        FocusCommand::Done => run_focus_done(conn),
        FocusCommand::SkipBreak => run_focus_skip_break(conn),
        FocusCommand::Pause => run_focus_pause(conn),
        FocusCommand::Resume => run_focus_resume(conn),
        FocusCommand::Stop => run_focus_stop(conn),
        FocusCommand::Status => run_focus_status(conn),
        FocusCommand::Stats { period } => run_focus_stats(conn, period),
        FocusCommand::History { task } => run_focus_history(conn, *task),
    }
}

fn run_focus_start(
    conn: &Connection,
    task: Option<u32>,
    cycles: u32,
    work: u32,
    short_break: u32,
    long_break: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(active) = tock_storage::repo::focus_repo::get_active(conn)? {
        eprintln!(
            "Focus session #{} is already active ({})",
            active.sid,
            active.state.as_str()
        );
        return Ok(());
    }

    let task_id = if let Some(sid) = task {
        tock_storage::repo::task_repo::get_by_sid(conn, sid)?.map(|task| task.id)
    } else {
        None
    };
    let default_config = tock_core::domain::focus::FocusConfig::default();
    let new = tock_core::domain::focus::NewFocusSession {
        task_id,
        project_id: None,
        planned_cycles: cycles,
        config: tock_core::domain::focus::FocusConfig {
            work_minutes: work,
            short_break_minutes: short_break,
            long_break_minutes: long_break,
            cycles_before_long_break: default_config.cycles_before_long_break,
        },
    };
    let session = tock_storage::repo::focus_repo::insert(conn, &new)?;
    notify(
        "Focus started",
        &format!("🍅 Work for {} minutes", session.config.work_minutes),
    );
    println!(
        "🍅 Focus #{} started — {} min work × {} cycles",
        session.sid, session.config.work_minutes, session.planned_cycles
    );
    Ok(())
}

fn run_focus_done(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let Some(active) = tock_storage::repo::focus_repo::get_active(conn)? else {
        println!("No active focus session");
        return Ok(());
    };

    let session = tock_storage::repo::focus_repo::complete_cycle(conn, active.sid)?;
    log_focus_time_block(conn, &active)?;
    if session.state.is_terminal() {
        notify(
            "Focus complete!",
            &format!("🎉 {} cycles done", session.completed_cycles),
        );
        println!(
            "🎉 Focus #{} complete! {} cycles",
            session.sid, session.completed_cycles
        );
        return Ok(());
    }

    let break_mins = if session.state == tock_core::domain::focus::FocusState::LongBreak {
        session.config.long_break_minutes
    } else {
        session.config.short_break_minutes
    };
    notify("Pomodoro done!", &format!("Take a {break_mins} min break"));
    println!(
        "✅ Cycle {}/{} done — {} min {} break",
        session.completed_cycles,
        session.planned_cycles,
        break_mins,
        session.state.as_str()
    );
    Ok(())
}

fn run_focus_skip_break(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let Some(active) = tock_storage::repo::focus_repo::get_active(conn)? else {
        println!("No active focus session");
        return Ok(());
    };

    let session = tock_storage::repo::focus_repo::start_work(conn, active.sid)?;
    println!(
        "⏩ Skipped break — working (cycle {})",
        session.completed_cycles + 1
    );
    Ok(())
}

fn run_focus_pause(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let Some(active) = tock_storage::repo::focus_repo::get_active(conn)? else {
        println!("No active focus session");
        return Ok(());
    };

    let session = tock_storage::repo::focus_repo::pause(conn, active.sid)?;
    println!("⏸ Focus #{} paused", session.sid);
    Ok(())
}

fn run_focus_resume(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let Some(active) = tock_storage::repo::focus_repo::get_active(conn)? else {
        println!("No active focus session");
        return Ok(());
    };

    let session = tock_storage::repo::focus_repo::resume(conn, active.sid)?;
    println!("▶ Focus #{} resumed", session.sid);
    Ok(())
}

fn run_focus_stop(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let Some(active) = tock_storage::repo::focus_repo::get_active(conn)? else {
        println!("No active focus session");
        return Ok(());
    };

    let session = tock_storage::repo::focus_repo::abort(conn, active.sid)?;
    println!(
        "⏹ Focus #{} aborted after {} cycles",
        session.sid, session.completed_cycles
    );
    Ok(())
}

fn run_focus_status(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let Some(session) = tock_storage::repo::focus_repo::get_active(conn)? else {
        println!("No active focus session");
        return Ok(());
    };

    let elapsed = time::OffsetDateTime::now_utc() - session.started_at;
    println!(
        "🍅 Focus #{} — {} ({}/{})",
        session.sid,
        session.state.as_str(),
        session.completed_cycles,
        session.planned_cycles
    );
    println!("   Elapsed: {}", format_duration(elapsed));
    println!(
        "   Config: {} work / {} short / {} long",
        session.config.work_minutes,
        session.config.short_break_minutes,
        session.config.long_break_minutes
    );
    Ok(())
}

fn run_focus_stats(conn: &Connection, period: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (from, to) = period_range(period);
    let sessions = tock_storage::repo::focus_repo::list_range(conn, &from, &to)?;
    let total_cycles: u32 = sessions
        .iter()
        .map(|session| session.completed_cycles)
        .sum();
    let completed = sessions
        .iter()
        .filter(|session| session.state == tock_core::domain::focus::FocusState::Completed)
        .count();
    let aborted = sessions
        .iter()
        .filter(|session| session.state == tock_core::domain::focus::FocusState::Aborted)
        .count();
    let total_work_mins: u32 = sessions
        .iter()
        .map(|session| session.completed_cycles * session.config.work_minutes)
        .sum();
    println!("Focus stats: {period}");
    println!("  Completed: {total_cycles} cycles ({completed} sessions)");
    println!("  Aborted:   {aborted} sessions");
    println!(
        "  Focus time: {}h {}m",
        total_work_mins / 60,
        total_work_mins % 60
    );
    Ok(())
}

fn run_focus_history(conn: &Connection, task_sid: u32) -> Result<(), Box<dyn std::error::Error>> {
    let task =
        tock_storage::repo::task_repo::get_by_sid(conn, task_sid)?.ok_or("task not found")?;
    let sessions = tock_storage::repo::focus_repo::list_for_task(conn, task.id)?;
    let blocks = tock_storage::repo::time_block_repo::list_for_task(conn, task.id)?;
    println!("Focus history for task #{} — {}", task.sid, task.title);
    println!("\n  Focus sessions ({}):", sessions.len());
    for s in &sessions {
        let cycles = format!("{}/{}", s.completed_cycles, s.planned_cycles);
        println!(
            "    #{:<4}  {:<10}  {:<7}  {}",
            s.sid,
            s.state.as_str(),
            cycles,
            format_time(s.started_at),
        );
    }
    println!("\n  Time blocks ({}):", blocks.len());
    for b in &blocks {
        let dur = b
            .duration()
            .map_or_else(|| "running".to_string(), format_duration);
        println!(
            "    #{:<4}  {:<10}  {:<8}  {}",
            b.sid,
            b.source.as_str(),
            dur,
            format_time(b.start_ts),
        );
    }
    Ok(())
}

fn log_focus_time_block(
    conn: &Connection,
    session: &tock_core::domain::focus::FocusSession,
) -> Result<(), Box<dyn std::error::Error>> {
    let end_ts = time::OffsetDateTime::now_utc();
    let start_ts = end_ts - time::Duration::minutes(i64::from(session.config.work_minutes));
    let title = focus_time_block_title(conn, session)?;
    let block = tock_core::domain::time_block::NewTimeBlock {
        title,
        task_id: session.task_id,
        project_id: session.project_id,
        notes: None,
        source: tock_core::domain::time_block::BlockSource::Pomodoro,
    };
    let _ = tock_storage::repo::time_block_repo::insert_completed(conn, &block, start_ts, end_ts)?;
    Ok(())
}

fn focus_time_block_title(
    conn: &Connection,
    session: &tock_core::domain::focus::FocusSession,
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(task_id) = session.task_id
        && let Some(task) = tock_storage::repo::task_repo::get_by_id(conn, task_id)?
    {
        return Ok(task.title);
    }

    Ok(format!("Focus #{}", session.sid))
}

fn run_time_cmd(
    conn: &Connection,
    cmd: &commands::time::TimeCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        TimeCommand::Start { words } => run_time_start(conn, words),
        TimeCommand::Stop => run_time_stop(conn),
        TimeCommand::Resume => run_time_resume(conn),
        TimeCommand::Current => run_time_current(conn),
        TimeCommand::Blocks { period, json } => run_time_blocks(conn, period, *json),
        TimeCommand::Report { period, json } => run_time_report(conn, period, *json),
        TimeCommand::Edit {
            sid,
            title,
            notes,
            start,
            end,
            task,
            billable,
        } => run_time_edit(
            conn,
            *sid,
            title.as_deref(),
            notes.as_deref(),
            start.as_deref(),
            end.as_deref(),
            *task,
            *billable,
        ),
    }
}

fn run_time_start(conn: &Connection, words: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (title, task_id) = resolve_time_start_input(conn, words)?;

    if let Some(running) = tock_storage::repo::time_block_repo::get_current(conn)? {
        tock_storage::repo::time_block_repo::stop(conn, running.sid)?;
        println!("Stopped #{} — {}", running.sid, running.title);
    }

    let new_block = tock_core::domain::time_block::NewTimeBlock {
        title,
        task_id,
        project_id: None,
        notes: None,
        source: tock_core::domain::time_block::BlockSource::Timer,
    };
    let block = tock_storage::repo::time_block_repo::insert(conn, &new_block)?;
    println!(
        "Started #{} — {} ({})",
        block.sid,
        block.title,
        format_time(block.start_ts)
    );
    Ok(())
}

fn run_time_stop(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(running) = tock_storage::repo::time_block_repo::get_current(conn)? {
        let block = tock_storage::repo::time_block_repo::stop(conn, running.sid)?;
        let duration = block.duration().map_or_else(String::new, format_duration);
        println!("Stopped #{} — {} ({})", block.sid, block.title, duration);
    } else {
        println!("No timer running");
    }
    Ok(())
}

fn run_time_resume(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let block = tock_storage::repo::time_block_repo::resume(conn)?;
    println!("Resumed #{} — {}", block.sid, block.title);
    Ok(())
}

fn run_time_current(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(block) = tock_storage::repo::time_block_repo::get_current(conn)? {
        let elapsed = time::OffsetDateTime::now_utc() - block.start_ts;
        println!(
            "#{} — {} (running {})",
            block.sid,
            block.title,
            format_duration(elapsed)
        );
    } else {
        println!("No timer running");
    }
    Ok(())
}

fn run_time_blocks(
    conn: &Connection,
    period: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (from, to) = period_range(period);
    let blocks = tock_storage::repo::time_block_repo::list_range(conn, &from, &to)?;
    if json {
        let payload: Result<Vec<serde_json::Value>, time::error::Format> = blocks
            .iter()
            .map(|block| {
                Ok(serde_json::json!({
                    "sid": block.sid,
                    "title": &block.title,
                    "start": format_timestamp_full(block.start_ts)?,
                    "duration": block
                        .duration()
                        .map_or_else(|| String::from("running"), format_duration),
                }))
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload?)?);
    } else {
        println!(
            "{:>4}  {:<30}  {:<20}  {:>8}",
            "SID", "Title", "Started", "Duration"
        );
        for block in &blocks {
            let duration = block
                .duration()
                .map_or_else(|| String::from("running"), format_duration);
            println!(
                "{:>4}  {:<30}  {:<20}  {:>8}",
                block.sid,
                truncate_str(&block.title, 30),
                format_time(block.start_ts),
                duration
            );
        }
        println!("\n{} block(s)", blocks.len());
    }
    Ok(())
}

fn run_time_report(
    conn: &Connection,
    period: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (from, to) = period_range(period);
    let blocks = tock_storage::repo::time_block_repo::list_range(conn, &from, &to)?;
    let mut by_title = std::collections::BTreeMap::<String, i64>::new();
    let mut total_secs = 0_i64;
    for block in &blocks {
        if let Some(duration) = block.duration() {
            let seconds = duration.whole_seconds();
            *by_title.entry(block.title.clone()).or_default() += seconds;
            total_secs += seconds;
        }
    }

    if json {
        let entries: Vec<_> = by_title
            .iter()
            .map(|(title, seconds)| {
                serde_json::json!({
                    "title": title,
                    "duration": format_duration_secs(*seconds),
                })
            })
            .collect();
        let payload = serde_json::json!({
            "period": period,
            "total": format_duration_secs(total_secs),
            "entries": entries,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("Time report: {period}");
        println!("{:<40}  {:>10}", "Title", "Duration");
        println!("{}", "-".repeat(52));
        for (title, seconds) in &by_title {
            println!(
                "{:<40}  {:>10}",
                truncate_str(title, 40),
                format_duration_secs(*seconds)
            );
        }
        println!("{}", "-".repeat(52));
        println!("{:<40}  {:>10}", "Total", format_duration_secs(total_secs));
    }
    Ok(())
}

fn resolve_time_start_input(
    conn: &Connection,
    words: &[String],
) -> Result<(String, Option<uuid::Uuid>), Box<dyn std::error::Error>> {
    let Some(first) = words.first() else {
        return Ok((String::from("Untitled"), None));
    };

    if let Ok(sid) = first.parse::<u32>()
        && let Some(task) = tock_storage::repo::task_repo::get_by_sid(conn, sid)?
    {
        return Ok((task.title.clone(), Some(task.id)));
    }

    let new_task = commands::add::parse_add_input(words);
    let task = tock_storage::repo::task_repo::insert(conn, &new_task)?;
    println!("Created task #{} — {}", task.sid, task.title);
    Ok((task.title.clone(), Some(task.id)))
}

fn format_time(t: time::OffsetDateTime) -> String {
    format!("{:02}:{:02}:{:02}", t.hour(), t.minute(), t.second())
}

fn format_timestamp_full(t: time::OffsetDateTime) -> Result<String, time::error::Format> {
    t.format(&Rfc3339)
}

fn format_duration(duration: time::Duration) -> String {
    format_duration_secs(duration.whole_seconds())
}

fn format_duration_secs(total_secs: i64) -> String {
    let sign = if total_secs < 0 { "-" } else { "" };
    let abs = total_secs.unsigned_abs();
    let hours = abs / 3_600;
    let minutes = (abs % 3_600) / 60;
    let seconds = abs % 60;
    format!("{sign}{hours}:{minutes:02}:{seconds:02}")
}

fn truncate_str(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }

    if s.chars().count() <= max {
        return s.to_string();
    }

    let mut truncated = s.chars().take(max - 1).collect::<String>();
    truncated.push('…');
    truncated
}

fn period_range(period: &str) -> (String, String) {
    let now = time::OffsetDateTime::now_utc();
    let today = now.date();
    match period {
        "today" => {
            let tomorrow = today + time::Duration::days(1);
            (
                format!(
                    "{:04}-{:02}-{:02}T00:00:00Z",
                    today.year(),
                    u8::from(today.month()),
                    today.day()
                ),
                format!(
                    "{:04}-{:02}-{:02}T00:00:00Z",
                    tomorrow.year(),
                    u8::from(tomorrow.month()),
                    tomorrow.day()
                ),
            )
        }
        "week" => {
            let weekday_num = today.weekday().number_days_from_monday();
            let monday = today - time::Duration::days(i64::from(weekday_num));
            let next_monday = monday + time::Duration::days(7);
            (
                format!(
                    "{:04}-{:02}-{:02}T00:00:00Z",
                    monday.year(),
                    u8::from(monday.month()),
                    monday.day()
                ),
                format!(
                    "{:04}-{:02}-{:02}T00:00:00Z",
                    next_monday.year(),
                    u8::from(next_monday.month()),
                    next_monday.day()
                ),
            )
        }
        "month" => {
            let from = format!(
                "{:04}-{:02}-01T00:00:00Z",
                today.year(),
                u8::from(today.month())
            );
            let to = if today.month() == time::Month::December {
                format!("{:04}-01-01T00:00:00Z", today.year() + 1)
            } else {
                format!(
                    "{:04}-{:02}-01T00:00:00Z",
                    today.year(),
                    u8::from(today.month()) + 1
                )
            };
            (from, to)
        }
        _ => (
            String::from("2000-01-01T00:00:00Z"),
            String::from("2100-01-01T00:00:00Z"),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_time_edit(
    conn: &Connection,
    sid: u32,
    title: Option<&str>,
    notes: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
    task: Option<u32>,
    billable: Option<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let task_id = match task {
        Some(tsid) => {
            let t =
                tock_storage::repo::task_repo::get_by_sid(conn, tsid)?.ok_or("task not found")?;
            Some(Some(t.id))
        }
        None => None,
    };
    let patch = tock_core::domain::time_block::TimeBlockPatch {
        title: title.map(String::from),
        notes: notes.map(|n| Some(n.to_string())),
        start: start.map(String::from),
        end: end.map(|e| Some(e.to_string())),
        task_id,
        billable,
    };
    let block = tock_storage::repo::time_block_repo::update(conn, sid, &patch)?;
    println!("Updated block #{} — {}", block.sid, block.title);
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

fn run_uda_cmd(conn: &Connection, cmd: &UdaCommand) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        UdaCommand::Add {
            key,
            r#type,
            label,
            default,
        } => {
            let type_name = r#type.as_str();
            let uda_type =
                tock_core::domain::uda::UdaType::from_str_opt(r#type).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("unsupported UDA type: {type_name}"),
                    )
                })?;
            let definition = tock_core::domain::uda::UdaDefinition {
                key: key.clone(),
                uda_type,
                label: label.clone(),
                default: default.clone(),
            };
            tock_storage::repo::uda_repo::add_definition(conn, &definition)?;
            println!(
                "Created UDA '{}' ({})",
                definition.key,
                definition.uda_type.as_str()
            );
        }
        UdaCommand::List => {
            let definitions = tock_storage::repo::uda_repo::list_definitions(conn)?;
            for definition in &definitions {
                println!(
                    "{:<20}  {:<8}  {:<20}  {}",
                    definition.key,
                    definition.uda_type.as_str(),
                    definition.label.as_deref().unwrap_or("—"),
                    definition.default.as_deref().unwrap_or("—")
                );
            }
            println!("\n{} UDA definition(s)", definitions.len());
        }
        UdaCommand::Rm { key } => {
            tock_storage::repo::uda_repo::remove_definition(conn, key)?;
            println!("Removed UDA '{key}'");
        }
    }
    Ok(())
}

fn run_report_cmd(
    conn: &Connection,
    cmd: &commands::report::ReportCommand,
    active_context: Option<&ActiveContext>,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        commands::report::ReportCommand::Define {
            name,
            query,
            sort,
            columns,
        } => {
            let cols = columns
                .split(',')
                .map(str::trim)
                .filter(|column| !column.is_empty())
                .map(str::to_owned)
                .collect();
            let new = tock_core::domain::report::NewReport {
                name: name.clone(),
                query: query.clone(),
                sort: sort.clone(),
                columns: cols,
            };
            let report = tock_storage::repo::report_repo::insert(conn, &new)?;
            println!("Defined report '{}' — query: {}", report.name, report.query);
        }
        commands::report::ReportCommand::List => {
            let reports = tock_storage::repo::report_repo::list(conn)?;
            if reports.is_empty() {
                println!("No saved reports. Examples:");
                println!("  tock report define overdue --query '+OVERDUE' --sort deadline");
                println!(
                    "  tock report define urgent  --query 'status:pending priority:H' --sort urgency"
                );
                println!(
                    "  tock report define work    --query 'tag:work status:pending' --columns sid,title,deadline"
                );
            } else {
                for report in &reports {
                    let sort_info = report.sort.as_deref().unwrap_or("urgency");
                    println!(
                        "  {:<20}  query: {:<30}  sort: {}",
                        report.name, report.query, sort_info
                    );
                }
                println!("\n{} report(s)", reports.len());
            }
        }
        commands::report::ReportCommand::Show { name, json } => {
            let Some(report) = tock_storage::repo::report_repo::get_by_name(conn, name)? else {
                eprintln!("report '{name}' not found");
                return Ok(());
            };
            let today = today_string();
            let query_args = report.query.split_whitespace().collect::<Vec<_>>();
            let base_filter = tock_parse::filter::parse_filter(&query_args, &today);
            let filter = combine_with_active_context(base_filter, active_context, &today);
            let mut filtered = filter_tasks(
                conn,
                tock_storage::repo::task_repo::list(conn, false)?,
                &filter,
            )?;
            sort_report_tasks(&mut filtered, report.sort.as_deref());
            if *json {
                println!("{}", format_tasks(&filtered, OutputFormat::Json));
            } else {
                println!("── {} ──", report.name);
                let rendered = format_report_tasks(&filtered, &report.columns);
                print_task_listing(
                    &rendered,
                    filtered.len(),
                    OutputFormat::Table,
                    active_context.map(|ctx| ctx.name.as_str()),
                );
            }
        }
        commands::report::ReportCommand::Rm { name } => {
            tock_storage::repo::report_repo::delete(conn, name)?;
            println!("Deleted report '{name}'");
        }
    }
    Ok(())
}

fn run_view_cmd(
    conn: &Connection,
    name: &str,
    json: bool,
    global_format: &str,
    active_context: Option<&ActiveContext>,
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

    let filter = combine_with_active_context(view.filter.clone(), active_context, &today_str);
    let filtered = filter_tasks(
        conn,
        tock_storage::repo::task_repo::list(conn, false)?,
        &filter,
    )?;

    let format = selected_output_format(global_format, json);
    let rendered = format_tasks(&filtered, format);
    if matches!(format, OutputFormat::Json) {
        println!("{rendered}");
    } else {
        println!("── {} ({}) ──", view.name, view.description);
        print_task_listing(
            &rendered,
            filtered.len(),
            format,
            active_context.map(|ctx| ctx.name.as_str()),
        );
    }
    Ok(())
}

fn sort_report_tasks(tasks: &mut [Task], sort: Option<&str>) {
    match sort.map(str::trim).filter(|field| !field.is_empty()) {
        Some(field) if field.eq_ignore_ascii_case("deadline") => tasks.sort_by(|left, right| {
            compare_optional_text(left.deadline.as_deref(), right.deadline.as_deref())
                .then_with(|| left.sid.cmp(&right.sid))
        }),
        Some(field) if field.eq_ignore_ascii_case("created") => tasks.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.sid.cmp(&right.sid))
        }),
        Some(field) if field.eq_ignore_ascii_case("sid") => {
            tasks.sort_by_key(|t| t.sid);
        }
        _ => tasks.sort_by(|left, right| {
            right
                .urgency
                .partial_cmp(&left.urgency)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.sid.cmp(&right.sid))
        }),
    }
}

fn compare_optional_text(left: Option<&str>, right: Option<&str>) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn format_report_tasks(tasks: &[Task], columns: &[String]) -> String {
    let normalized_columns = normalized_report_columns(columns);
    let widths = normalized_columns
        .iter()
        .map(|column| {
            let header_width = report_column_label(column).chars().count();
            let cell_width = tasks
                .iter()
                .map(|task| report_column_value(task, column).chars().count())
                .max()
                .unwrap_or(0);
            std::cmp::max(
                header_width,
                std::cmp::min(cell_width, report_column_max_width(column)),
            )
        })
        .collect::<Vec<_>>();

    let header = normalized_columns
        .iter()
        .zip(&widths)
        .map(|(column, width)| format_report_cell(report_column_label(column), *width))
        .collect::<Vec<_>>()
        .join("  ");
    let rows = tasks
        .iter()
        .map(|task| {
            normalized_columns
                .iter()
                .zip(&widths)
                .map(|(column, width)| {
                    format_report_cell(&report_column_value(task, column), *width)
                })
                .collect::<Vec<_>>()
                .join("  ")
        })
        .collect::<Vec<_>>();

    let mut lines = Vec::with_capacity(rows.len().saturating_add(1));
    lines.push(header);
    lines.extend(rows);
    lines.join("\n")
}

fn normalized_report_columns(columns: &[String]) -> Vec<String> {
    let normalized = columns
        .iter()
        .map(|column| column.trim().to_ascii_lowercase())
        .filter(|column| !column.is_empty())
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        vec![
            String::from("sid"),
            String::from("priority"),
            String::from("status"),
            String::from("title"),
            String::from("deadline"),
            String::from("tags"),
        ]
    } else {
        normalized
    }
}

fn report_column_label(column: &str) -> &'static str {
    match column {
        "sid" => "SID",
        "priority" => "Priority",
        "status" => "Status",
        "title" => "Title",
        "deadline" => "Deadline",
        "tags" => "Tags",
        "urgency" => "Urgency",
        "created" | "created_at" => "Created",
        "modified" | "modified_at" => "Modified",
        "start" | "start_date" => "Start",
        "evening" => "Evening",
        _ => "Value",
    }
}

fn report_column_max_width(column: &str) -> usize {
    match column {
        "sid" => 4,
        "urgency" | "evening" => 7,
        "priority" => 8,
        "status" => 9,
        "deadline" | "created" | "created_at" | "modified" | "modified_at" | "start"
        | "start_date" => 12,
        "tags" => 30,
        "title" => 40,
        _ => 20,
    }
}

fn report_column_value(task: &Task, column: &str) -> String {
    match column {
        "sid" => task.sid.to_string(),
        "priority" => task
            .priority
            .map_or_else(String::new, |priority| priority.as_char().to_string()),
        "status" => task.status.as_str().to_owned(),
        "title" => task.title.clone(),
        "deadline" => task.deadline.clone().unwrap_or_default(),
        "tags" => task
            .tags
            .iter()
            .map(|tag| format!("#{tag}"))
            .collect::<Vec<_>>()
            .join(" "),
        "urgency" => format!("{:.2}", task.urgency),
        "created" | "created_at" => today_string_for(task.created_at),
        "modified" | "modified_at" => today_string_for(task.modified_at),
        "start" | "start_date" => task.start_date.clone().unwrap_or_default(),
        "evening" => {
            if task.evening {
                String::from("yes")
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

fn format_report_cell(value: &str, width: usize) -> String {
    format!("{:<width$}", truncate_str(value, width))
}

fn parse_habit_direction_arg(
    direction: &str,
) -> Result<tock_core::domain::habit::HabitDirection, std::io::Error> {
    tock_core::domain::habit::HabitDirection::from_str_opt(direction).ok_or_else(|| {
        std::io::Error::other(format!(
            "invalid habit direction '{direction}' (expected 'build' or 'break')"
        ))
    })
}

fn format_habit_status_line(habit: &tock_core::domain::habit::Habit) -> String {
    format!(
        "{} {}  L{} ({})  {} {}d  best {}d  {}",
        habit.direction_emoji(),
        habit.title,
        habit.level,
        habit.level_name(),
        habit.streak_label(),
        habit.streak_current,
        habit.streak_best,
        habit_cadence_display(&habit.cadence)
    )
}

#[allow(dead_code)]
const fn habit_icon(_direction: tock_core::domain::habit::HabitDirection) -> &'static str {
    // Kept for backward compat; prefer habit.direction_emoji() instead.
    "📖"
}

fn format_habit_xp(level: u32, xp: u32) -> String {
    let (_, next) = habit_level_window(level, xp);
    format!("{xp}/{next} XP")
}

fn habit_level_window(level: u32, xp: u32) -> (u32, u32) {
    const LEVEL_THRESHOLDS: [u32; 7] = [0, 5, 13, 34, 89, 233, 610];
    let current_index = usize::try_from(level.saturating_sub(1))
        .unwrap_or(LEVEL_THRESHOLDS.len() - 1)
        .min(LEVEL_THRESHOLDS.len() - 1);
    let start = LEVEL_THRESHOLDS[current_index];
    let end = LEVEL_THRESHOLDS
        .get(current_index + 1)
        .copied()
        .unwrap_or_else(|| xp.max(start));
    (start, end)
}

fn format_stack_delay(delay_s: u32) -> String {
    if delay_s >= 3_600 && delay_s.is_multiple_of(3_600) {
        format!("{}h", delay_s / 3_600)
    } else if delay_s >= 60 && delay_s.is_multiple_of(60) {
        format!("{}m", delay_s / 60)
    } else {
        format!("{delay_s}s")
    }
}

fn next_due_text(
    habit: &tock_core::domain::habit::Habit,
    entries: &[tock_core::domain::habit::HabitEntry],
    sid_by_id: &std::collections::HashMap<uuid::Uuid, u32>,
) -> String {
    if let Some(parent_id) = habit.stack_after {
        let parent = sid_by_id.get(&parent_id).map_or_else(
            || String::from("stacked habit"),
            |sid| format!("after #{sid}"),
        );
        return if habit.stack_delay_s == 0 {
            parent
        } else {
            format!("{parent} + {}", format_stack_delay(habit.stack_delay_s))
        };
    }

    let today = time::OffsetDateTime::now_utc().date();
    let done_today = entries
        .iter()
        .any(|entry| entry.occurred_at.date() == today);
    match ParsedCadence::from_json(&habit.cadence) {
        Some(ParsedCadence::Daily) => {
            if done_today {
                String::from("tomorrow")
            } else {
                String::from("today")
            }
        }
        Some(ParsedCadence::WeeklyTarget { times_per_week }) => {
            let week_start =
                today - time::Duration::days(i64::from(today.weekday().number_days_from_monday()));
            let week_end = week_start + time::Duration::days(7);
            let count = entries
                .iter()
                .filter(|entry| {
                    let date = entry.occurred_at.date();
                    !entry.slip && date >= week_start && date < week_end
                })
                .count();
            if count >= usize::from(times_per_week) {
                String::from("next week")
            } else {
                String::from("this week")
            }
        }
        Some(ParsedCadence::SpecificDays { days }) => {
            if !done_today && days.contains(&today.weekday()) {
                return String::from("today");
            }
            for offset in 1_i64..=7 {
                let date = today + time::Duration::days(offset);
                if days.contains(&date.weekday()) {
                    return format_due_date(date, today);
                }
            }
            String::from("scheduled")
        }
        Some(ParsedCadence::EveryNDays { n }) => {
            let Some(last) = entries
                .iter()
                .find(|entry| !entry.slip)
                .map(|entry| entry.occurred_at.date())
            else {
                return String::from("today");
            };
            let due = last + time::Duration::days(i64::from(n));
            if due <= today {
                String::from("today")
            } else {
                format_due_date(due, today)
            }
        }
        None => {
            if done_today {
                String::from("logged today")
            } else {
                String::from("by cadence")
            }
        }
    }
}

fn format_due_date(date: time::Date, today: time::Date) -> String {
    if date == today {
        String::from("today")
    } else if date == today + time::Duration::days(1) {
        String::from("tomorrow")
    } else {
        format!(
            "{:04}-{:02}-{:02}",
            date.year(),
            u8::from(date.month()),
            date.day()
        )
    }
}

fn habit_cadence_display(raw: &str) -> String {
    ParsedCadence::from_json(raw).map_or_else(|| raw.to_owned(), |cadence| cadence.display())
}

const fn selected_output_format(global_format: &str, json: bool) -> OutputFormat {
    if json {
        OutputFormat::Json
    } else {
        OutputFormat::from_str(global_format)
    }
}

fn print_task_listing(
    rendered: &str,
    count: usize,
    format: OutputFormat,
    active_context: Option<&str>,
) {
    if !matches!(format, OutputFormat::Json)
        && let Some(active_context) = active_context
    {
        println!("[ctx: {active_context}]");
    }
    if !rendered.is_empty() {
        println!("{rendered}");
    }
    if !matches!(format, OutputFormat::Json) {
        println!("\n{count} task(s)");
    }
}

fn load_active_context(
    conn: &Connection,
) -> Result<Option<ActiveContext>, Box<dyn std::error::Error>> {
    let Some(name) = tock_storage::repo::context_repo::get_active(conn)? else {
        return Ok(None);
    };
    let Some(filter) = tock_storage::repo::context_repo::get_filter(conn, &name)? else {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("active context '{name}' is missing"),
        )));
    };
    Ok(Some(ActiveContext { name, filter }))
}

fn combine_with_active_context(
    base: tock_parse::filter::Filter,
    active_context: Option<&ActiveContext>,
    today: &str,
) -> tock_parse::filter::Filter {
    let Some(active_context) = active_context else {
        return base;
    };
    let context_args = active_context.filter.split_whitespace().collect::<Vec<_>>();
    let context_filter = tock_parse::filter::parse_filter(&context_args, today);
    tock_parse::filter::Filter::And(vec![context_filter, base])
}

fn filter_tasks(
    conn: &Connection,
    tasks: Vec<Task>,
    filter: &tock_parse::filter::Filter,
) -> Result<Vec<Task>, Box<dyn std::error::Error>> {
    let states = build_task_filter_states(conn, &tasks)?;
    Ok(tasks
        .into_iter()
        .filter(|task| {
            let state = states.get(&task.id).copied().unwrap_or_default();
            tock_parse::filter::matches(filter, &TaskFilterable { task, state })
        })
        .collect())
}

fn build_task_filter_states(
    conn: &Connection,
    tasks: &[Task],
) -> Result<HashMap<Uuid, TaskFilterState>, Box<dyn std::error::Error>> {
    let mut states = HashMap::with_capacity(tasks.len());
    for task in tasks {
        let dependents = tock_storage::repo::task_repo::get_dependents(conn, task.id)?;
        states.insert(
            task.id,
            TaskFilterState {
                is_blocked: tock_storage::repo::task_repo::is_blocked(conn, task.id)?,
                is_blocking: !dependents.is_empty(),
            },
        );
    }
    Ok(states)
}

fn print_task_show(
    conn: &Connection,
    task: &Task,
    format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let dependencies = related_task_summaries(conn, &task.depends_on)?;
    let dependents = related_task_summaries(
        conn,
        &tock_storage::repo::task_repo::get_dependents(conn, task.id)?,
    )?;

    if matches!(format, OutputFormat::Json) {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "task": {
                    "sid": task.sid,
                    "title": &task.title,
                    "status": task.status.as_str(),
                    "priority": task.priority.map(|priority| priority.as_char().to_string()),
                    "deadline": task.deadline.as_deref(),
                    "start_date": task.start_date.as_deref(),
                    "recurrence": task.recurrence.as_deref(),
                    "parent_id": task.parent_id.map(|id| id.to_string()),
                    "depends_on": task.depends_on.iter().map(Uuid::to_string).collect::<Vec<_>>(),
                    "tags": &task.tags,
                    "udas": &task.udas.0,
                    "notes": task.notes.as_deref(),
                    "created_at": task.created_at.to_string(),
                    "modified_at": task.modified_at.to_string(),
                    "done_at": task.done_at.map(|value| value.to_string()),
                    "cancelled_at": task.cancelled_at.map(|value| value.to_string()),
                },
                "dependencies": dependencies,
                "dependents": dependents,
            }))?
        );
        return Ok(());
    }

    println!("{}", format_task_detail(task, format));
    if let Some(recurrence) = task.recurrence.as_deref() {
        println!("  Recurs:   {}", describe_recurrence(recurrence));
    }
    if !dependencies.is_empty() {
        println!(
            "  Depends:  {}",
            format_related_task_summaries(&dependencies)
        );
    }
    if !dependents.is_empty() {
        println!("  Blocking: {}", format_related_task_summaries(&dependents));
    }
    Ok(())
}

fn related_task_summaries(
    conn: &Connection,
    ids: &[Uuid],
) -> Result<Vec<RelatedTaskSummary>, Box<dyn std::error::Error>> {
    let mut summaries = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(task) = tock_storage::repo::task_repo::get_by_id(conn, *id)? {
            summaries.push(RelatedTaskSummary {
                sid: task.sid,
                title: task.title,
                status: task.status.as_str().to_string(),
            });
        }
    }
    Ok(summaries)
}

fn format_related_task_summaries(tasks: &[RelatedTaskSummary]) -> String {
    tasks
        .iter()
        .map(|task| format!("#{} [{}] {}", task.sid, task.status, task.title))
        .collect::<Vec<_>>()
        .join(", ")
}

fn describe_recurrence(recurrence_json: &str) -> String {
    use tock_core::domain::recurrence::{RecurrenceMode, RecurrencePattern, RecurrenceSpec};

    let Some(spec) = RecurrenceSpec::from_json(recurrence_json) else {
        return recurrence_json.to_string();
    };
    let pattern = match spec.pattern {
        RecurrencePattern::Daily => "daily".to_string(),
        RecurrencePattern::Weekly => "weekly".to_string(),
        RecurrencePattern::Monthly => "monthly".to_string(),
        RecurrencePattern::Yearly => "yearly".to_string(),
        RecurrencePattern::EveryNDays(days) => format!("every {days} days"),
        RecurrencePattern::EveryNWeeks(weeks) => format!("every {weeks} weeks"),
    };
    let mode = match spec.mode {
        RecurrenceMode::Periodic => "periodic",
        RecurrenceMode::Chained => "chained",
    };
    format!("{pattern} ({mode})")
}

fn new_task_to_hook_json(task: &NewTask) -> String {
    serde_json::json!({
        "title": &task.title,
        "notes": &task.notes,
        "status": task.status.map(|status| status.as_str()),
        "project_id": task.project_id.map(|id| id.to_string()),
        "area_id": task.area_id.map(|id| id.to_string()),
        "heading_id": task.heading_id.map(|id| id.to_string()),
        "parent_id": task.parent_id.map(|id| id.to_string()),
        "start_date": &task.start_date,
        "deadline": &task.deadline,
        "recurrence": &task.recurrence,
        "priority": task.priority.map(|priority| priority.as_char().to_string()),
        "evening": task.evening,
        "udas": &task.udas.0,
        "tags": &task.tags,
    })
    .to_string()
}

fn new_task_from_hook_json(original: &NewTask, hook_json: &str) -> Result<NewTask, std::io::Error> {
    let value: serde_json::Value = serde_json::from_str(hook_json)
        .map_err(|error| hook_json_error(format!("invalid on-add hook JSON: {error}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| hook_json_error("on-add hook JSON must be an object"))?;

    let mut task = original.clone();
    if let Some(title) = object.get("title") {
        task.title = title
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| hook_json_error("hook field 'title' must be a string"))?;
    }
    apply_optional_string_field(object, "notes", &mut task.notes)?;
    if let Some(status) = object.get("status") {
        task.status = match status {
            serde_json::Value::Null => None,
            serde_json::Value::String(raw) => Some(
                TaskStatus::from_str_opt(raw)
                    .ok_or_else(|| hook_json_error(format!("invalid task status '{raw}'")))?,
            ),
            _ => {
                return Err(hook_json_error(
                    "hook field 'status' must be a string or null",
                ));
            }
        };
    }
    apply_optional_uuid_field(object, "project_id", &mut task.project_id)?;
    apply_optional_uuid_field(object, "area_id", &mut task.area_id)?;
    apply_optional_uuid_field(object, "heading_id", &mut task.heading_id)?;
    apply_optional_uuid_field(object, "parent_id", &mut task.parent_id)?;
    apply_optional_string_field(object, "start_date", &mut task.start_date)?;
    apply_optional_string_field(object, "deadline", &mut task.deadline)?;
    apply_optional_string_field(object, "recurrence", &mut task.recurrence)?;
    if let Some(priority) = object.get("priority") {
        task.priority = match priority {
            serde_json::Value::Null => None,
            serde_json::Value::String(raw) => Some(
                tock_core::domain::task::Priority::from_str_opt(raw)
                    .ok_or_else(|| hook_json_error(format!("invalid task priority '{raw}'")))?,
            ),
            _ => {
                return Err(hook_json_error(
                    "hook field 'priority' must be a string or null",
                ));
            }
        };
    }
    if let Some(evening) = object.get("evening") {
        task.evening = evening
            .as_bool()
            .ok_or_else(|| hook_json_error("hook field 'evening' must be a boolean"))?;
    }
    if let Some(udas) = object.get("udas") {
        if udas.is_object() {
            task.udas = tock_core::domain::uda::UdaValues::from_json(&udas.to_string());
        } else {
            return Err(hook_json_error("hook field 'udas' must be an object"));
        }
    }
    if let Some(tags) = object.get("tags") {
        let values = tags
            .as_array()
            .ok_or_else(|| hook_json_error("hook field 'tags' must be an array"))?;
        task.tags = values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| hook_json_error("hook field 'tags' must contain only strings"))
            })
            .collect::<Result<Vec<_>, _>>()?;
    }

    Ok(task)
}

fn task_to_hook_json(task: &Task) -> String {
    serde_json::json!({
        "sid": task.sid,
        "title": &task.title,
        "status": task.status.as_str(),
        "priority": task.priority.map(|priority| priority.as_char().to_string()),
        "deadline": &task.deadline,
        "tags": &task.tags,
    })
    .to_string()
}

fn explain_task_urgency(task: &Task, is_blocked: bool) -> Vec<(String, f64, f64, f64)> {
    let now = time::OffsetDateTime::now_utc();
    let today = today_string_for(now);
    let input = tock_core::domain::urgency::UrgencyInput {
        priority: task.priority.map(|priority| priority.as_char()),
        deadline: task.deadline.as_deref(),
        start_date: task.start_date.as_deref(),
        tags: &task.tags,
        has_project: task.project_id.is_some(),
        is_blocked,
        created_at_days_ago: task_age_days(task.created_at, now),
        today: &today,
    };
    tock_core::domain::urgency::explain(
        &input,
        &tock_core::domain::urgency::UrgencyConfig::default(),
    )
}

fn apply_optional_string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    target: &mut Option<String>,
) -> Result<(), std::io::Error> {
    if let Some(value) = object.get(field) {
        *target = match value {
            serde_json::Value::Null => None,
            serde_json::Value::String(raw) => Some(raw.clone()),
            _ => {
                return Err(hook_json_error(format!(
                    "hook field '{field}' must be a string or null"
                )));
            }
        };
    }
    Ok(())
}

fn apply_optional_uuid_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    target: &mut Option<Uuid>,
) -> Result<(), std::io::Error> {
    if let Some(value) = object.get(field) {
        *target = match value {
            serde_json::Value::Null => None,
            serde_json::Value::String(raw) => Some(Uuid::parse_str(raw).map_err(|error| {
                hook_json_error(format!("invalid UUID in hook field '{field}': {error}"))
            })?),
            _ => {
                return Err(hook_json_error(format!(
                    "hook field '{field}' must be a UUID string or null"
                )));
            }
        };
    }
    Ok(())
}

fn hook_json_error(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into())
}

fn task_age_days(created_at: time::OffsetDateTime, now: time::OffsetDateTime) -> f64 {
    let days = (now - created_at).whole_days().clamp(0, 365);
    f64::from(u16::try_from(days).unwrap_or(365))
}

fn today_string() -> String {
    today_string_for(time::OffsetDateTime::now_utc())
}

fn today_string_for(now: time::OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    )
}

/// Adapter: makes `Task` implement `tock_parse::filter::Filterable`.
struct TaskFilterable<'a> {
    task: &'a tock_core::domain::task::Task,
    state: TaskFilterState,
}

impl tock_parse::filter::Filterable for TaskFilterable<'_> {
    fn status(&self) -> &str {
        self.task.status.as_str()
    }

    fn tags(&self) -> &[String] {
        &self.task.tags
    }

    fn priority(&self) -> Option<char> {
        self.task
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
        self.task.deadline.as_deref()
    }

    fn start_date(&self) -> Option<&str> {
        self.task.start_date.as_deref()
    }

    fn is_evening(&self) -> bool {
        self.task.evening
    }

    fn is_deleted(&self) -> bool {
        self.task.deleted_at.is_some()
    }

    fn is_blocked(&self) -> bool {
        self.state.is_blocked
    }

    fn is_blocking(&self) -> bool {
        self.state.is_blocking
    }

    fn uda_value(&self, key: &str) -> Option<String> {
        self.task.udas.get_str(key)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod cli_tests {
    use super::Cli;
    use clap::CommandFactory as _;

    /// Validate the full clap command tree, including the new `sync`,
    /// `onboard`, and `device` subcommands and their arguments.
    #[test]
    fn command_tree_is_valid() {
        Cli::command().debug_assert();
    }

    /// The sync command group must expose the documented subcommands.
    #[test]
    fn sync_subcommands_parse() {
        let cmd = Cli::command();
        let sync = cmd
            .get_subcommands()
            .find(|c| c.get_name() == "sync")
            .expect("sync subcommand present");
        let names: Vec<&str> = sync
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect();
        assert!(names.contains(&"conflicts"), "sync conflicts present");
        assert!(names.contains(&"resolve"), "sync resolve present");
    }
}
