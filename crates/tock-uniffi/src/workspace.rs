//! `Workspace` — the top-level handle exposed to Swift via `UniFFI`.
//!
//! Wraps an [`OpenVault`] in `Arc<Mutex<Option<…>>>` so that:
//! * Multiple `Arc<Workspace>` handles can be shared across threads
//!   (`UniFFI` objects are `Arc`-wrapped).
//! * The vault can be `lock()`-ed (consumed), after which all methods
//!   return [`TockError::Locked`].
//!
//! All repository methods serialise through the single `Mutex`. This is
//! correct for a single-writer `SQLite` connection. A future async layer
//! (ADR-005 §4.3.1) will wrap this behind a `tokio` current-thread
//! runtime so Swift callers can `await` without blocking the main thread.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tock_core::domain::area::NewArea;
use tock_core::domain::focus::{FocusConfig, NewFocusSession};
use tock_core::domain::habit::NewHabit;
use tock_core::domain::project::NewProject;
use tock_core::domain::task::TaskStatus;
use tock_core::domain::time_block::{BlockSource, NewTimeBlock};
use tock_storage::OpenVault;

use crate::convert::parse_optional_uuid;
use crate::error::TockError;
use crate::types::{
    TockArea, TockFocusSession, TockHabit, TockHabitEntry, TockNewArea, TockNewFocusSession,
    TockNewHabit, TockNewProject, TockNewTask, TockNewTimeBlock, TockProject, TockTag, TockTask,
    TockTaskPatch, TockTimeBlock,
};

// ── Top-level constructors ───────────────────────────────────────────

/// Create a new vault at `path` protected by `password` and return an
/// open `Workspace` handle.
///
/// # Errors
///
/// Returns [`TockError::StorageError`] on I/O or `SQLite` failures,
/// [`TockError::InvalidCredentials`] if the vault metadata cannot be
/// initialised.
#[allow(clippy::needless_pass_by_value)]
#[uniffi::export]
pub fn init_workspace(path: String, password: Vec<u8>) -> Result<Arc<Workspace>, TockError> {
    let vault = tock_storage::init(&PathBuf::from(&path), &password)?;
    Ok(Arc::new(Workspace {
        vault: Mutex::new(Some(vault)),
        path,
    }))
}

/// Open an existing vault at `path` with `password` and return a
/// `Workspace` handle.
///
/// # Errors
///
/// Returns [`TockError::VaultNotFound`] if the file does not exist,
/// [`TockError::InvalidCredentials`] for a wrong password, and
/// [`TockError::StorageError`] for other failures.
#[allow(clippy::needless_pass_by_value)]
#[uniffi::export]
pub fn open_workspace(path: String, password: Vec<u8>) -> Result<Arc<Workspace>, TockError> {
    let vault = tock_storage::open(&PathBuf::from(&path), &password).map_err(|e| match e {
        tock_storage::Error::NotFound => TockError::VaultNotFound,
        other => TockError::from(other),
    })?;
    Ok(Arc::new(Workspace {
        vault: Mutex::new(Some(vault)),
        path,
    }))
}

// ── Workspace object ─────────────────────────────────────────────────

/// Top-level handle to a tock workspace (an unlocked vault).
///
/// All domain operations (tasks, projects, habits, time, focus) are
/// methods on this object. The vault can be locked via [`lock()`],
/// after which all methods return [`TockError::Locked`].
#[derive(uniffi::Object)]
pub struct Workspace {
    vault: Mutex<Option<OpenVault>>,
    path: String,
}

impl core::fmt::Debug for Workspace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let locked = self.vault.lock().is_ok_and(|g| g.is_none());
        f.debug_struct("Workspace")
            .field("path", &self.path)
            .field("locked", &locked)
            .finish_non_exhaustive()
    }
}

/// Internal helper: acquire the mutex and require the vault to be open.
macro_rules! with_vault {
    ($self:expr, |$conn:ident| $body:expr) => {{
        let guard = $self.vault.lock().map_err(|_| TockError::InternalError {
            message: "workspace mutex poisoned".into(),
        })?;
        let vault = guard.as_ref().ok_or(TockError::Locked)?;
        let $conn = vault.connection();
        $body
    }};
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::significant_drop_tightening
)]
#[uniffi::export]
impl Workspace {
    // ── Vault lifecycle ──────────────────────────────────────────────

    /// Lock the workspace, zeroing key material and closing the
    /// database connection.
    ///
    /// After this call every method returns [`TockError::Locked`].
    pub fn lock(&self) -> Result<(), TockError> {
        let mut guard = self.vault.lock().map_err(|_| TockError::InternalError {
            message: "workspace mutex poisoned".into(),
        })?;
        if let Some(vault) = guard.take() {
            vault.lock();
        }
        Ok(())
    }

    /// Path the vault was opened from.
    pub fn vault_path(&self) -> String {
        self.path.clone()
    }

    // ── Tasks ────────────────────────────────────────────────────────

    /// Add a new task.
    pub fn add_task(&self, input: TockNewTask) -> Result<TockTask, TockError> {
        let core_input = input.to_core()?;
        with_vault!(self, |conn| {
            tock_storage::repo::task_repo::insert(conn, &core_input)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Get a task by its short ID.
    pub fn get_task(&self, sid: u32) -> Result<Option<TockTask>, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::task_repo::get_by_sid(conn, sid)
                .map(|o| o.map(Into::into))
                .map_err(Into::into)
        })
    }

    /// List all non-deleted tasks ordered by urgency.
    pub fn list_tasks(&self) -> Result<Vec<TockTask>, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::task_repo::list(conn, false)
                .map(|v| v.into_iter().map(Into::into).collect())
                .map_err(Into::into)
        })
    }

    /// Modify a task by its short ID.
    pub fn modify_task(&self, sid: u32, patch: TockTaskPatch) -> Result<TockTask, TockError> {
        let core_patch = patch.to_core()?;
        with_vault!(self, |conn| {
            tock_storage::repo::task_repo::update(conn, sid, &core_patch)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Mark a task as done.
    pub fn complete_task(&self, sid: u32) -> Result<TockTask, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::task_repo::set_status(conn, sid, TaskStatus::Done)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Mark a task as cancelled.
    pub fn cancel_task(&self, sid: u32) -> Result<TockTask, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::task_repo::set_status(conn, sid, TaskStatus::Cancelled)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Soft-delete a task.
    pub fn delete_task(&self, sid: u32) -> Result<(), TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::task_repo::soft_delete(conn, sid).map_err(Into::into)
        })
    }

    // ── Projects ─────────────────────────────────────────────────────

    /// Add a new project.
    pub fn add_project(&self, input: TockNewProject) -> Result<TockProject, TockError> {
        let core_input = NewProject {
            name: input.name,
            notes: input.notes,
            area_id: parse_optional_uuid(input.area_id.as_deref())?,
            deadline: input.deadline,
        };
        with_vault!(self, |conn| {
            tock_storage::repo::project_repo::insert(conn, &core_input)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Get a project by its short ID.
    pub fn get_project(&self, sid: u32) -> Result<Option<TockProject>, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::project_repo::get_by_sid(conn, sid)
                .map(|o| o.map(Into::into))
                .map_err(Into::into)
        })
    }

    /// List active projects.
    pub fn list_projects(&self) -> Result<Vec<TockProject>, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::project_repo::list(conn, false)
                .map(|v| v.into_iter().map(Into::into).collect())
                .map_err(Into::into)
        })
    }

    // ── Areas ────────────────────────────────────────────────────────

    /// Add a new area.
    pub fn add_area(&self, input: TockNewArea) -> Result<TockArea, TockError> {
        let core_input = NewArea {
            name: input.name,
            color: input.color,
        };
        with_vault!(self, |conn| {
            tock_storage::repo::area_repo::insert(conn, &core_input)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// List active areas.
    pub fn list_areas(&self) -> Result<Vec<TockArea>, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::area_repo::list(conn, false)
                .map(|v| v.into_iter().map(Into::into).collect())
                .map_err(Into::into)
        })
    }

    // ── Tags ─────────────────────────────────────────────────────────

    /// List all tags in the vault.
    pub fn list_tags(&self) -> Result<Vec<TockTag>, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::tag_repo::list_all(conn)
                .map(|v| v.into_iter().map(Into::into).collect())
                .map_err(Into::into)
        })
    }

    // ── Time tracking ────────────────────────────────────────────────

    /// Start a new running timer.
    pub fn start_timer(&self, input: TockNewTimeBlock) -> Result<TockTimeBlock, TockError> {
        let task_id = if let Some(task_sid) = input.task_sid {
            with_vault!(self, |conn| {
                tock_storage::repo::task_repo::get_by_sid(conn, task_sid)
                    .map_err(TockError::from)
                    .and_then(|t| t.map(|t| t.id).ok_or(TockError::NotFound))
                    .map(Some)
            })?
        } else {
            None
        };

        let core_input = NewTimeBlock {
            title: input.title,
            task_id,
            project_id: parse_optional_uuid(input.project_id.as_deref())?,
            notes: input.notes,
            source: BlockSource::Timer,
        };
        with_vault!(self, |conn| {
            tock_storage::repo::time_block_repo::insert(conn, &core_input)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Stop the currently running timer by SID.
    pub fn stop_timer(&self, sid: u32) -> Result<TockTimeBlock, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::time_block_repo::stop(conn, sid)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Get the currently running time block, if any.
    pub fn current_timer(&self) -> Result<Option<TockTimeBlock>, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::time_block_repo::get_current(conn)
                .map(|o| o.map(Into::into))
                .map_err(Into::into)
        })
    }

    /// Resume the most recently stopped timer.
    pub fn resume_timer(&self) -> Result<TockTimeBlock, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::time_block_repo::resume(conn)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// List all time blocks (including running ones).
    pub fn list_time_blocks(&self) -> Result<Vec<TockTimeBlock>, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::time_block_repo::list(conn, true)
                .map(|v| v.into_iter().map(Into::into).collect())
                .map_err(Into::into)
        })
    }

    // ── Focus sessions ───────────────────────────────────────────────

    /// Start a new focus (Pomodoro) session.
    pub fn start_focus(&self, input: TockNewFocusSession) -> Result<TockFocusSession, TockError> {
        let task_id = if let Some(task_sid) = input.task_sid {
            with_vault!(self, |conn| {
                tock_storage::repo::task_repo::get_by_sid(conn, task_sid)
                    .map_err(TockError::from)
                    .and_then(|t| t.map(|t| t.id).ok_or(TockError::NotFound))
                    .map(Some)
            })?
        } else {
            None
        };

        let core_input = NewFocusSession {
            task_id,
            project_id: parse_optional_uuid(input.project_id.as_deref())?,
            planned_cycles: input.planned_cycles,
            config: FocusConfig::from(input.config),
        };
        with_vault!(self, |conn| {
            tock_storage::repo::focus_repo::insert(conn, &core_input)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Get the currently active (non-terminal) focus session, if any.
    pub fn focus_status(&self) -> Result<Option<TockFocusSession>, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::focus_repo::get_active(conn)
                .map(|o| o.map(Into::into))
                .map_err(Into::into)
        })
    }

    /// Complete the current work cycle and transition to the next state.
    pub fn complete_focus_cycle(&self, sid: u32) -> Result<TockFocusSession, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::focus_repo::complete_cycle(conn, sid)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Skip the current break and start the next work cycle.
    pub fn skip_focus_break(&self, sid: u32) -> Result<TockFocusSession, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::focus_repo::start_work(conn, sid)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Pause a running focus session.
    pub fn pause_focus(&self, sid: u32) -> Result<TockFocusSession, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::focus_repo::pause(conn, sid)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Resume a paused focus session.
    pub fn resume_focus(&self, sid: u32) -> Result<TockFocusSession, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::focus_repo::resume(conn, sid)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Abort a focus session.
    pub fn abort_focus(&self, sid: u32) -> Result<TockFocusSession, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::focus_repo::abort(conn, sid)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// Mark a focus session as completed.
    pub fn finish_focus(&self, sid: u32) -> Result<TockFocusSession, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::focus_repo::finish(conn, sid)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    // ── Habits ───────────────────────────────────────────────────────

    /// Add a new habit.
    pub fn add_habit(&self, input: TockNewHabit) -> Result<TockHabit, TockError> {
        let core_input = NewHabit {
            title: input.title,
            identity: input.identity,
            cue: input.cue,
            craving: input.craving,
            response: input.response,
            reward: input.reward,
            direction: input.direction.into(),
            cadence: input.cadence,
            minimum: input.minimum,
            stack_after: input.stack_after,
            stack_delay_s: input.stack_delay_s,
            area_id: parse_optional_uuid(input.area_id.as_deref())?,
            project_id: parse_optional_uuid(input.project_id.as_deref())?,
        };
        with_vault!(self, |conn| {
            tock_storage::repo::habit_repo::insert(conn, &core_input)
                .map(Into::into)
                .map_err(Into::into)
        })
    }

    /// List active habits.
    pub fn list_habits(&self) -> Result<Vec<TockHabit>, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::habit_repo::list(conn, false)
                .map(|v| v.into_iter().map(Into::into).collect())
                .map_err(Into::into)
        })
    }

    /// Get a habit by its short ID.
    pub fn get_habit(&self, sid: u32) -> Result<Option<TockHabit>, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::habit_repo::get_by_sid(conn, sid)
                .map(|o| o.map(Into::into))
                .map_err(Into::into)
        })
    }

    /// Log a habit entry (completion or slip).
    pub fn log_habit(
        &self,
        habit_sid: u32,
        amount: String,
        notes: Option<String>,
        slip: bool,
    ) -> Result<TockHabitEntry, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::habit_repo::log_entry(
                conn,
                habit_sid,
                &amount,
                notes.as_deref(),
                slip,
            )
            .map(Into::into)
            .map_err(Into::into)
        })
    }

    /// Archive a habit by its short ID.
    pub fn archive_habit(&self, sid: u32) -> Result<(), TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::habit_repo::archive(conn, sid).map_err(Into::into)
        })
    }
}
