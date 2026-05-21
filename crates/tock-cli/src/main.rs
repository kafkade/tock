//! # tock CLI
//!
//! Command-line interface for tock — unified task, habit, time, and
//! focus engine. Phase 1 implements the task management commands.

mod commands;
mod display;
mod notify;
mod tracing_setup;

use std::path::PathBuf;
use std::process;

use clap::{CommandFactory, Parser};
use commands::{Commands, focus::FocusCommand, time::TimeCommand};
use display::{OutputFormat, format_task_detail, format_tasks};
use notify::notify;
use rusqlite::Connection;
use time::format_description::well_known::Rfc3339;

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
        Commands::Time(args) => run_time_cmd(conn, &args.command),
        Commands::Focus(args) => run_focus_cmd(conn, &args.command),
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
                // Auto-stop any active focus session linked to this task.
                if let Some(active) = tock_storage::repo::focus_repo::get_active(conn)? {
                    if active.task_id == Some(task.id) {
                        let _ = tock_storage::repo::focus_repo::abort(conn, active.sid);
                        println!("  (auto-stopped focus session #{})", active.sid);
                    }
                }
                // Auto-stop any running time block linked to this task.
                if let Some(running) = tock_storage::repo::time_block_repo::get_current(conn)? {
                    if running.task_id == Some(task.id) {
                        let _ = tock_storage::repo::time_block_repo::stop(conn, running.sid);
                        println!("  (auto-stopped timer #{})", running.sid);
                    }
                }
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

    if let Ok(sid) = first.parse::<u32>() {
        if let Some(task) = tock_storage::repo::task_repo::get_by_sid(conn, sid)? {
            return Ok((task.title.clone(), Some(task.id)));
        }
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
