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

/// Run the interactive terminal user interface.
///
/// # Errors
/// Returns an error if terminal setup, event handling, drawing, or task
/// repository operations fail.
pub fn run(conn: &Connection) -> Result<(), Box<dyn Error>> {
    let cleanup = TerminalCleanup::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let mut state = state::AppState::new(conn)?;

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
    match code {
        KeyCode::Char('q') | KeyCode::Esc => state.should_quit = true,
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.should_quit = true;
        }
        KeyCode::Tab | KeyCode::Right => state.next_pane(),
        KeyCode::BackTab | KeyCode::Left => state.prev_pane(),
        KeyCode::Char('1') => state.active_pane = state::ActivePane::Sidebar,
        KeyCode::Char('2') => state.active_pane = state::ActivePane::TaskList,
        KeyCode::Char('3') => state.active_pane = state::ActivePane::Detail,
        KeyCode::Char('j') | KeyCode::Down => state.move_down(),
        KeyCode::Char('k') | KeyCode::Up => state.move_up(),
        KeyCode::Char('g') => state.move_to_top(),
        KeyCode::Char('G') => state.move_to_bottom(),
        KeyCode::Enter => state.activate_selected(conn)?,
        KeyCode::Char('d') if matches!(state.active_pane, state::ActivePane::TaskList) => {
            state.complete_selected_task(conn)?;
        }
        KeyCode::Char('x') if matches!(state.active_pane, state::ActivePane::TaskList) => {
            state.delete_selected_task(conn)?;
        }
        KeyCode::Char('r') => state.reload_tasks(conn)?,
        KeyCode::Char('?') => state.show_help = !state.show_help,
        _ => {}
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
