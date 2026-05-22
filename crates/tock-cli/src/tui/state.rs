//! TUI application state.

use std::collections::HashMap;
use std::error::Error;

use rusqlite::Connection;
use time::OffsetDateTime;
use tock_core::domain::task::{Task, TaskStatus};
use tock_parse::filter::{self, Filter, Filterable};
use uuid::Uuid;

/// The pane that currently has keyboard focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivePane {
    /// The sidebar showing built-in views and projects.
    Sidebar,
    /// The middle pane showing the filtered task list.
    TaskList,
    /// The detail pane showing the selected task.
    Detail,
}

/// A selectable item in the sidebar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SidebarItem {
    /// A built-in view such as `inbox` or `today`.
    View(String),
    /// A project identified by SID and name.
    Project(u32, String),
}

/// Mutable application state for the TUI.
#[derive(Debug)]
pub struct AppState {
    /// The pane that currently has focus.
    pub active_pane: ActivePane,
    /// Built-in views followed by active projects.
    pub sidebar_items: Vec<SidebarItem>,
    /// Selected sidebar row.
    pub sidebar_selected: usize,
    /// Tasks currently shown in the task list.
    pub tasks: Vec<Task>,
    /// Selected task row.
    pub task_selected: usize,
    /// Whether the application should exit.
    pub should_quit: bool,
    /// Status text shown at the bottom of the screen.
    pub status_message: Option<String>,
    project_names: HashMap<Uuid, String>,
}

impl AppState {
    /// Build the initial TUI state.
    ///
    /// # Errors
    /// Returns an error if the repository queries fail.
    pub(crate) fn new(conn: &Connection) -> Result<Self, Box<dyn Error>> {
        let mut state = Self {
            active_pane: ActivePane::Sidebar,
            sidebar_items: Vec::new(),
            sidebar_selected: 0,
            tasks: Vec::new(),
            task_selected: 0,
            should_quit: false,
            status_message: Some(String::from(
                "Tab: panes · j/k: move · Enter: select · d: done · x: delete · r: refresh · q: quit",
            )),
            project_names: HashMap::new(),
        };
        state.reload_sidebar(conn)?;
        state.reload_tasks_with_selection(conn, None)?;
        Ok(state)
    }

    /// Move focus to the next pane.
    pub(crate) const fn next_pane(&mut self) {
        self.active_pane = match self.active_pane {
            ActivePane::Sidebar => ActivePane::TaskList,
            ActivePane::TaskList => ActivePane::Detail,
            ActivePane::Detail => ActivePane::Sidebar,
        };
    }

    /// Move focus to the previous pane.
    pub(crate) const fn prev_pane(&mut self) {
        self.active_pane = match self.active_pane {
            ActivePane::Sidebar => ActivePane::Detail,
            ActivePane::TaskList => ActivePane::Sidebar,
            ActivePane::Detail => ActivePane::TaskList,
        };
    }

    /// Move the active selection up by one row.
    pub(crate) const fn move_up(&mut self) {
        match self.active_pane {
            ActivePane::Sidebar => {
                self.sidebar_selected = self.sidebar_selected.saturating_sub(1);
            }
            ActivePane::TaskList => {
                self.task_selected = self.task_selected.saturating_sub(1);
            }
            ActivePane::Detail => {}
        }
    }

    /// Move the active selection down by one row.
    pub(crate) fn move_down(&mut self) {
        match self.active_pane {
            ActivePane::Sidebar => {
                self.sidebar_selected = next_index(self.sidebar_selected, self.sidebar_items.len());
            }
            ActivePane::TaskList => {
                self.task_selected = next_index(self.task_selected, self.tasks.len());
            }
            ActivePane::Detail => {}
        }
    }

    /// Move the active selection to the first row.
    pub(crate) const fn move_to_top(&mut self) {
        match self.active_pane {
            ActivePane::Sidebar => self.sidebar_selected = 0,
            ActivePane::TaskList => self.task_selected = 0,
            ActivePane::Detail => {}
        }
    }

    /// Move the active selection to the last row.
    pub(crate) const fn move_to_bottom(&mut self) {
        match self.active_pane {
            ActivePane::Sidebar => {
                self.sidebar_selected = self.sidebar_items.len().saturating_sub(1);
            }
            ActivePane::TaskList => {
                self.task_selected = self.tasks.len().saturating_sub(1);
            }
            ActivePane::Detail => {}
        }
    }

    /// Activate the current selection.
    ///
    /// # Errors
    /// Returns an error if refreshing task data fails.
    pub(crate) fn activate_selected(&mut self, conn: &Connection) -> Result<(), Box<dyn Error>> {
        match self.active_pane {
            ActivePane::Sidebar => {
                self.reload_tasks_with_selection(conn, None)?;
                self.active_pane = ActivePane::TaskList;
                self.status_message = Some(format!(
                    "Loaded {} task(s) from {}",
                    self.tasks.len(),
                    self.current_sidebar_label()
                ));
            }
            ActivePane::TaskList => {
                if let Some(task_sid) = self.selected_task().map(|task| task.sid) {
                    self.active_pane = ActivePane::Detail;
                    self.status_message = Some(format!("Viewing task #{task_sid}"));
                } else {
                    self.status_message = Some(String::from("No task selected"));
                }
            }
            ActivePane::Detail => {}
        }
        Ok(())
    }

    /// Mark the selected task as done.
    ///
    /// # Errors
    /// Returns an error if the repository update fails.
    pub(crate) fn complete_selected_task(
        &mut self,
        conn: &Connection,
    ) -> Result<(), Box<dyn Error>> {
        let Some(task) = self.selected_task().cloned() else {
            self.status_message = Some(String::from("No task selected"));
            return Ok(());
        };

        let updated = tock_storage::repo::task_repo::set_status(conn, task.sid, TaskStatus::Done)?;
        self.reload_tasks_with_selection(conn, Some(updated.sid))?;
        self.status_message = Some(format!(
            "Completed task #{} — {}",
            updated.sid, updated.title
        ));
        Ok(())
    }

    /// Soft-delete the selected task.
    ///
    /// # Errors
    /// Returns an error if the repository update fails.
    pub(crate) fn delete_selected_task(&mut self, conn: &Connection) -> Result<(), Box<dyn Error>> {
        let Some(task) = self.selected_task().cloned() else {
            self.status_message = Some(String::from("No task selected"));
            return Ok(());
        };

        tock_storage::repo::task_repo::soft_delete(conn, task.sid)?;
        self.reload_tasks_with_selection(conn, Some(task.sid))?;
        self.status_message = Some(format!("Deleted task #{}", task.sid));
        Ok(())
    }

    /// Reload the task list for the currently selected sidebar item.
    ///
    /// # Errors
    /// Returns an error if repository queries fail.
    pub(crate) fn reload_tasks(&mut self, conn: &Connection) -> Result<(), Box<dyn Error>> {
        let selected_sid = self.selected_task().map(|task| task.sid);
        self.reload_tasks_with_selection(conn, selected_sid)
    }

    /// Return the selected task, if any.
    #[must_use]
    pub(crate) fn selected_task(&self) -> Option<&Task> {
        self.tasks.get(self.task_selected)
    }

    /// Return the current sidebar label for titles and status text.
    #[must_use]
    pub(crate) fn current_sidebar_label(&self) -> String {
        match self.sidebar_items.get(self.sidebar_selected) {
            Some(SidebarItem::View(name)) => name.clone(),
            Some(SidebarItem::Project(sid, name)) => format!("#{sid} {name}"),
            None => String::from("inbox"),
        }
    }

    /// Return the display name for a project id.
    #[must_use]
    pub(crate) fn project_name(&self, project_id: &Uuid) -> Option<&str> {
        self.project_names.get(project_id).map(String::as_str)
    }

    fn reload_sidebar(&mut self, conn: &Connection) -> Result<(), Box<dyn Error>> {
        let previous_selection = self.sidebar_items.get(self.sidebar_selected).cloned();
        let projects = tock_storage::repo::project_repo::list(conn, false)?;

        self.project_names = projects
            .iter()
            .map(|project| (project.id, project.name.clone()))
            .collect();

        let mut items = crate::commands::views::all_views(&today_string())
            .into_iter()
            .map(|view| SidebarItem::View(view.name.to_string()))
            .collect::<Vec<_>>();
        items.extend(
            projects
                .into_iter()
                .map(|project| SidebarItem::Project(project.sid, project.name)),
        );

        self.sidebar_selected = previous_selection
            .as_ref()
            .and_then(|selected| items.iter().position(|item| item == selected))
            .unwrap_or_else(|| self.sidebar_selected.min(items.len().saturating_sub(1)));
        self.sidebar_items = items;
        Ok(())
    }

    fn reload_tasks_with_selection(
        &mut self,
        conn: &Connection,
        selected_sid: Option<u32>,
    ) -> Result<(), Box<dyn Error>> {
        self.reload_sidebar(conn)?;
        let filter = self.current_filter();
        let tasks = tock_storage::repo::task_repo::list(conn, false)?;
        self.tasks = filter_tasks(tasks, &filter, &self.project_names);
        self.restore_task_selection(selected_sid);
        Ok(())
    }

    fn current_filter(&self) -> Filter {
        let today = today_string();
        match self.sidebar_items.get(self.sidebar_selected) {
            Some(SidebarItem::View(name)) => crate::commands::views::all_views(&today)
                .into_iter()
                .find(|view| view.name == name)
                .map_or_else(
                    || crate::commands::views::inbox().filter,
                    |view| view.filter,
                ),
            Some(SidebarItem::Project(_, name)) => Filter::Project(name.clone()),
            None => crate::commands::views::inbox().filter,
        }
    }

    fn restore_task_selection(&mut self, selected_sid: Option<u32>) {
        if let Some(sid) = selected_sid
            && let Some(index) = self.tasks.iter().position(|task| task.sid == sid)
        {
            self.task_selected = index;
            return;
        }

        self.task_selected = self.task_selected.min(self.tasks.len().saturating_sub(1));
        if self.tasks.is_empty() {
            self.task_selected = 0;
        }
    }
}

struct TaskFilterableView<'a> {
    task: &'a Task,
    project_names: &'a HashMap<Uuid, String>,
}

impl Filterable for TaskFilterableView<'_> {
    fn status(&self) -> &str {
        self.task.status.as_str()
    }

    fn tags(&self) -> &[String] {
        &self.task.tags
    }

    fn priority(&self) -> Option<char> {
        self.task.priority.map(|priority| priority.as_char())
    }

    fn project_name(&self) -> Option<&str> {
        self.task
            .project_id
            .as_ref()
            .and_then(|project_id| self.project_names.get(project_id).map(String::as_str))
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
        !self.task.depends_on.is_empty()
    }

    fn is_blocking(&self) -> bool {
        false
    }

    fn uda_value(&self, key: &str) -> Option<String> {
        self.task.udas.get_str(key)
    }
}

fn filter_tasks(
    tasks: Vec<Task>,
    filter: &Filter,
    project_names: &HashMap<Uuid, String>,
) -> Vec<Task> {
    tasks
        .into_iter()
        .filter(|task| {
            filter::matches(
                filter,
                &TaskFilterableView {
                    task,
                    project_names,
                },
            )
        })
        .collect()
}

fn next_index(current: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        current.saturating_add(1).min(len.saturating_sub(1))
    }
}

fn today_string() -> String {
    format_date(OffsetDateTime::now_utc())
}

fn format_date(now: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;
    use tock_core::domain::project::NewProject;
    use tock_core::domain::task::{NewTask, TaskStatus};

    use super::{ActivePane, AppState, SidebarItem};
    use crate::commands;
    use tock_storage::migrations;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    #[test]
    fn new_state_loads_views_projects_and_inbox_tasks() {
        let conn = test_conn();
        let project = tock_storage::repo::project_repo::insert(
            &conn,
            &NewProject {
                name: String::from("Launch"),
                notes: None,
                area_id: None,
                deadline: None,
            },
        )
        .expect("insert project");

        tock_storage::repo::task_repo::insert(
            &conn,
            &NewTask {
                title: String::from("Triage inbox"),
                status: Some(TaskStatus::Inbox),
                ..NewTask::default()
            },
        )
        .expect("insert inbox task");
        tock_storage::repo::task_repo::insert(
            &conn,
            &NewTask {
                title: String::from("Ship launch"),
                status: Some(TaskStatus::Pending),
                project_id: Some(project.id),
                ..NewTask::default()
            },
        )
        .expect("insert project task");

        let state = AppState::new(&conn).expect("build state");
        let expected_views = commands::views::all_views("2026-01-01").len();

        assert_eq!(state.sidebar_items.len(), expected_views + 1);
        assert_eq!(
            state.sidebar_items[0],
            SidebarItem::View(String::from("inbox"))
        );
        assert_eq!(
            state.sidebar_items.last(),
            Some(&SidebarItem::Project(project.sid, String::from("Launch")))
        );
        assert_eq!(state.tasks.len(), 1);
        assert_eq!(state.tasks[0].title, "Triage inbox");
    }

    #[test]
    fn activating_project_filters_tasks() {
        let conn = test_conn();
        let project = tock_storage::repo::project_repo::insert(
            &conn,
            &NewProject {
                name: String::from("Launch"),
                notes: None,
                area_id: None,
                deadline: None,
            },
        )
        .expect("insert project");

        tock_storage::repo::task_repo::insert(
            &conn,
            &NewTask {
                title: String::from("Project task"),
                status: Some(TaskStatus::Pending),
                project_id: Some(project.id),
                ..NewTask::default()
            },
        )
        .expect("insert project task");
        tock_storage::repo::task_repo::insert(
            &conn,
            &NewTask {
                title: String::from("Other task"),
                status: Some(TaskStatus::Pending),
                ..NewTask::default()
            },
        )
        .expect("insert other task");

        let mut state = AppState::new(&conn).expect("build state");
        state.sidebar_selected = state.sidebar_items.len().saturating_sub(1);
        state.activate_selected(&conn).expect("activate project");

        assert_eq!(state.active_pane, ActivePane::TaskList);
        assert_eq!(state.tasks.len(), 1);
        assert_eq!(state.tasks[0].title, "Project task");
    }

    #[test]
    fn complete_selected_task_reloads_current_view() {
        let conn = test_conn();
        let task = tock_storage::repo::task_repo::insert(
            &conn,
            &NewTask {
                title: String::from("Finish docs"),
                status: Some(TaskStatus::Inbox),
                ..NewTask::default()
            },
        )
        .expect("insert task");

        let mut state = AppState::new(&conn).expect("build state");
        state.complete_selected_task(&conn).expect("complete task");

        assert!(state.tasks.is_empty());
        let updated = tock_storage::repo::task_repo::get_by_sid(&conn, task.sid)
            .expect("fetch task")
            .expect("task exists");
        assert_eq!(updated.status, TaskStatus::Done);
    }

    #[test]
    fn pane_navigation_cycles() {
        let mut state = AppState {
            active_pane: ActivePane::Sidebar,
            sidebar_items: vec![SidebarItem::View(String::from("inbox"))],
            sidebar_selected: 0,
            tasks: Vec::new(),
            task_selected: 0,
            should_quit: false,
            status_message: None,
            project_names: std::collections::HashMap::new(),
        };

        state.next_pane();
        assert_eq!(state.active_pane, ActivePane::TaskList);
        state.next_pane();
        assert_eq!(state.active_pane, ActivePane::Detail);
        state.prev_pane();
        assert_eq!(state.active_pane, ActivePane::TaskList);
    }
}
