//! TUI rendering functions.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use tock_core::domain::task::TaskStatus;

use super::state::{ActivePane, AppState, SidebarItem};

/// Draw the entire TUI frame.
pub(super) fn draw(frame: &mut Frame, state: &AppState) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(24),
            Constraint::Min(40),
            Constraint::Length(36),
        ])
        .split(vertical[0]);

    draw_sidebar(frame, state, chunks[0]);
    draw_task_list(frame, state, chunks[1]);
    draw_detail(frame, state, chunks[2]);
    draw_status(frame, state, vertical[1]);
}

fn draw_sidebar(frame: &mut Frame, state: &AppState, area: Rect) {
    let items = state
        .sidebar_items
        .iter()
        .map(|item| match item {
            SidebarItem::View(name) => ListItem::new(name.clone()),
            SidebarItem::Project(sid, name) => ListItem::new(format!("#{sid} {name}")),
        })
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(
            Block::default()
                .title(format!(" {} ", crate::tr!("tui-sidebar-title")))
                .borders(Borders::ALL)
                .border_style(pane_border_style(matches!(
                    state.active_pane,
                    ActivePane::Sidebar
                ))),
        )
        .highlight_style(selection_style(matches!(
            state.active_pane,
            ActivePane::Sidebar
        )))
        .highlight_symbol("> ");

    let mut list_state = ListState::default();
    if !state.sidebar_items.is_empty() {
        list_state.select(Some(state.sidebar_selected));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_task_list(frame: &mut Frame, state: &AppState, area: Rect) {
    let items = state
        .tasks
        .iter()
        .map(|task| {
            let priority = task.priority.map_or(' ', |value| value.as_char());
            let status_icon = match task.status {
                TaskStatus::Done => '✓',
                TaskStatus::Cancelled => '✗',
                TaskStatus::Started => '▶',
                _ => '○',
            };
            let label = format!(
                "{:>3} {} {} {}{}",
                task.sid,
                status_icon,
                priority,
                task.title,
                if task.evening { " ☽" } else { "" }
            );
            let style = if task.status.is_closed() {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };
            ListItem::new(label).style(style)
        })
        .collect::<Vec<_>>();
    let title = format!(
        " {} ({}) ",
        state.current_sidebar_label(),
        state.tasks.len()
    );
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(pane_border_style(matches!(
                    state.active_pane,
                    ActivePane::TaskList
                ))),
        )
        .highlight_style(selection_style(matches!(
            state.active_pane,
            ActivePane::TaskList
        )))
        .highlight_symbol("> ");

    let mut list_state = ListState::default();
    if !state.tasks.is_empty() {
        list_state.select(Some(state.task_selected));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
}

#[allow(clippy::too_many_lines)]
fn draw_detail(frame: &mut Frame, state: &AppState, area: Rect) {
    let content = if state.show_help {
        let key_width = 18;
        [
            crate::tr!("tui-help-title"),
            String::new(),
            format!("{}:", crate::tr!("tui-help-section-navigation")),
            format!(
                "  {:<key_width$} {}",
                "Tab / Shift+Tab",
                crate::tr!("tui-help-switch-panes")
            ),
            format!(
                "  {:<key_width$} {}",
                "← / →",
                crate::tr!("tui-help-switch-panes")
            ),
            format!(
                "  {:<key_width$} {}",
                "1 / 2 / 3",
                crate::tr!("tui-help-jump-pane")
            ),
            format!(
                "  {:<key_width$} {}",
                "j / ↓",
                crate::tr!("tui-help-move-down")
            ),
            format!(
                "  {:<key_width$} {}",
                "k / ↑",
                crate::tr!("tui-help-move-up")
            ),
            format!("  {:<key_width$} {}", "g", crate::tr!("tui-help-go-top")),
            format!("  {:<key_width$} {}", "G", crate::tr!("tui-help-go-bottom")),
            format!(
                "  {:<key_width$} {}",
                "Enter",
                crate::tr!("tui-help-select-expand")
            ),
            String::new(),
            format!("{}:", crate::tr!("tui-help-section-actions")),
            format!("  {:<key_width$} {}", "d", crate::tr!("tui-help-mark-done")),
            format!(
                "  {:<key_width$} {}",
                "x",
                crate::tr!("tui-help-delete-task")
            ),
            format!("  {:<key_width$} {}", "r", crate::tr!("tui-help-refresh")),
            String::new(),
            format!("{}:", crate::tr!("tui-help-section-general")),
            format!(
                "  {:<key_width$} {}",
                "?",
                crate::tr!("tui-help-toggle-help")
            ),
            format!(
                "  {:<key_width$} {}",
                "q / Esc",
                crate::tr!("tui-help-quit")
            ),
            format!("  {:<key_width$} {}", "Ctrl+C", crate::tr!("tui-help-quit")),
        ]
        .join("\n")
    } else if let Some(task) = state.selected_task().cloned() {
        let mut lines = vec![
            format!("#{} {}", task.sid, task.title),
            String::new(),
            format!(
                "{}: {}",
                crate::tr!("tui-detail-status"),
                task.status.as_str()
            ),
            format!("{}: {:.2}", crate::tr!("tui-detail-urgency"), task.urgency),
        ];
        if let Some(project_id) = task.project_id.as_ref()
            && let Some(project_name) = state.project_name(project_id)
        {
            lines.push(format!(
                "{}: {project_name}",
                crate::tr!("tui-detail-project")
            ));
        }
        if let Some(priority) = task.priority {
            lines.push(format!(
                "{}: {}",
                crate::tr!("tui-detail-priority"),
                priority.as_char()
            ));
        }
        if let Some(start_date) = &task.start_date {
            lines.push(format!("{}: {start_date}", crate::tr!("tui-detail-start")));
        }
        if let Some(deadline) = &task.deadline {
            lines.push(format!("{}: {deadline}", crate::tr!("tui-detail-deadline")));
        }
        if task.evening {
            lines.push(crate::tr!("tui-detail-evening"));
        }
        if !task.tags.is_empty() {
            lines.push(format!(
                "{}: {}",
                crate::tr!("tui-detail-tags"),
                task.tags
                    .iter()
                    .map(|tag| format!("#{tag}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        if let Some(notes) = &task.notes {
            lines.push(String::new());
            lines.push(format!("{}:", crate::tr!("tui-detail-notes")));
            lines.push(notes.clone());
        }
        lines.join("\n")
    } else {
        crate::tr!("tui-no-task-selected")
    };

    let title = if state.show_help {
        format!(" {} ", crate::tr!("tui-pane-help"))
    } else {
        format!(" {} ", crate::tr!("tui-pane-detail"))
    };
    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(pane_border_style(matches!(
                    state.active_pane,
                    ActivePane::Detail
                ))),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn draw_status(frame: &mut Frame, state: &AppState, area: Rect) {
    let hint = state
        .status_message
        .clone()
        .unwrap_or_else(|| crate::tr!("tui-status-hint"));
    let status = Paragraph::new(hint).style(Style::default().fg(Color::Yellow));
    frame.render_widget(status, area);
}

fn pane_border_style(active: bool) -> Style {
    if active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn selection_style(active: bool) -> Style {
    let base = Style::default().bg(Color::DarkGray).fg(Color::White);
    if active {
        base.add_modifier(Modifier::BOLD)
    } else {
        base
    }
}
