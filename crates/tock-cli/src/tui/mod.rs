//! Terminal user interface for tock.

mod state;
mod ui;

use std::error::Error;
use std::io;
use std::time::Duration;

use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use rusqlite::Connection;

use crate::config::{Action, Config, Keymap};

/// Run the interactive terminal user interface.
///
/// # Errors
/// Returns an error if terminal setup, event handling, drawing, or task
/// repository operations fail.
pub fn run(
    conn: &Connection,
    cfg: &Config,
    urgency: &tock_core::domain::urgency::UrgencyConfig,
) -> Result<(), Box<dyn Error>> {
    let cleanup = TerminalCleanup::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let keymap = Keymap::from_config(&cfg.keys);
    let mut state = state::AppState::new(conn, urgency.clone(), keymap)?;

    loop {
        terminal.draw(|frame| ui::draw(frame, &state))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            handle_key(&mut state, conn, key.code, key.modifiers)?;
        }

        if state.should_quit {
            break;
        }
    }

    terminal.show_cursor()?;
    drop(terminal);
    drop(cleanup);
    Ok(())
}

fn handle_key(
    state: &mut state::AppState,
    conn: &Connection,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<(), Box<dyn Error>> {
    // Ctrl-C always quits, regardless of the configured keymap.
    if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
        state.should_quit = true;
        return Ok(());
    }

    // Configured keybindings take precedence over the navigational fallbacks.
    if let Some(action) = state.keymap.action(code) {
        return apply_action(state, conn, action);
    }

    // Built-in fallbacks for keys the config does not (re)bind.
    match code {
        KeyCode::Esc => state.should_quit = true,
        KeyCode::Right => state.next_pane(),
        KeyCode::Left => state.prev_pane(),
        KeyCode::Char('1') => state.active_pane = state::ActivePane::Sidebar,
        KeyCode::Char('2') => state.active_pane = state::ActivePane::TaskList,
        KeyCode::Char('3') => state.active_pane = state::ActivePane::Detail,
        KeyCode::Down => state.move_down(),
        KeyCode::Up => state.move_up(),
        _ => {}
    }
    Ok(())
}

fn apply_action(
    state: &mut state::AppState,
    conn: &Connection,
    action: Action,
) -> Result<(), Box<dyn Error>> {
    match action {
        Action::Quit => state.should_quit = true,
        Action::NextPane => state.next_pane(),
        Action::PrevPane => state.prev_pane(),
        Action::Down => state.move_down(),
        Action::Up => state.move_up(),
        Action::Top => state.move_to_top(),
        Action::Bottom => state.move_to_bottom(),
        Action::Activate => state.activate_selected(conn)?,
        Action::Complete if matches!(state.active_pane, state::ActivePane::TaskList) => {
            state.complete_selected_task(conn)?;
        }
        Action::Delete if matches!(state.active_pane, state::ActivePane::TaskList) => {
            state.delete_selected_task(conn)?;
        }
        Action::Complete | Action::Delete => {}
        Action::Reload => state.reload_tasks(conn)?,
        Action::Help => state.show_help = !state.show_help,
    }
    Ok(())
}

struct TerminalCleanup;

impl TerminalCleanup {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
    }
}
