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

use time::format_description::well_known::Rfc3339;
use tock_core::domain::area::NewArea;
use tock_core::domain::focus::{FocusConfig, NewFocusSession};
use tock_core::domain::habit::NewHabit;
use tock_core::domain::project::NewProject;
use tock_core::domain::task::TaskStatus;
use tock_core::domain::time_block::{BlockSource, NewTimeBlock};
use tock_core::event::DeviceId;
use tock_storage::{OpenVault, sync};
use tock_sync::pairing;
use tock_sync::transport::OnboardingBlob;
use tock_sync::wire;

use crate::convert::parse_optional_uuid;
use crate::error::TockError;
use crate::types::{
    TockArea, TockFocusSession, TockHabit, TockHabitEntry, TockNewArea, TockNewFocusSession,
    TockNewHabit, TockNewProject, TockNewTask, TockNewTimeBlock, TockPairingAcceptorInfo,
    TockPairingInvite, TockProject, TockSyncConflict, TockSyncDeviceInfo, TockSyncEventFrame,
    TockSyncIngestSummary, TockTag, TockTask, TockTaskPatch, TockTimeBlock,
};

// ── Top-level constructors ───────────────────────────────────────────

/// Result of initialising a fresh vault: the open workspace plus the
/// one-time Emergency-Kit string that encodes the generated account
/// Secret Key. The Secret Key is never stored in the vault or transmitted;
/// the caller MUST surface `secret_key` to the user exactly once.
#[derive(uniffi::Record)]
pub struct WorkspaceInit {
    /// The unlocked workspace handle.
    pub workspace: Arc<Workspace>,
    /// Emergency-Kit string (`A4-…`) encoding the account Secret Key.
    pub secret_key: String,
}

/// Create a new vault at `path` protected by `password` and return an
/// open `Workspace` handle together with the generated Secret Key.
///
/// # Errors
///
/// Returns [`TockError::StorageError`] on I/O or `SQLite` failures,
/// [`TockError::InvalidCredentials`] if the vault metadata cannot be
/// initialised.
#[allow(clippy::needless_pass_by_value)]
#[uniffi::export]
pub fn init_workspace(path: String, password: Vec<u8>) -> Result<WorkspaceInit, TockError> {
    let (vault, secret_key) = tock_storage::init(&PathBuf::from(&path), &password)?;
    let account_id = vault.header().account_id;
    let kit = secret_key.to_emergency_kit(account_id.as_bytes());
    let workspace = Arc::new(Workspace {
        vault: Mutex::new(Some(vault)),
        path,
    });
    Ok(WorkspaceInit {
        workspace,
        secret_key: kit,
    })
}

/// Start the accepter side of a device-pairing handshake.
///
/// Returns an object that owns the acceptor's ephemeral X25519 secret
/// until the onboarding blob is received and opened.
///
/// # Errors
///
/// Returns [`TockError::StorageError`] if secure randomness could not be
/// generated for the rendezvous device id or a storage-backed error from the
/// pairing bootstrap.
#[uniffi::export]
pub fn begin_pairing_accept() -> Result<Arc<PairingAcceptSession>, TockError> {
    let secret = pairing::accept_invite()?;
    let mut rendezvous_device_id = [0_u8; 16];
    tock_crypto::random::fill_random(&mut rendezvous_device_id).map_err(|e| {
        TockError::StorageError {
            message: e.to_string(),
        }
    })?;
    Ok(Arc::new(PairingAcceptSession {
        secret: Mutex::new(Some(secret)),
        rendezvous_device_id,
    }))
}

/// Open an existing vault at `path` with `password` and the account
/// `secret_key` (the `A4-…` Emergency-Kit string) and return a
/// `Workspace` handle.
///
/// # Errors
///
/// Returns [`TockError::VaultNotFound`] if the file does not exist,
/// [`TockError::InvalidCredentials`] for a wrong password or Secret Key,
/// and [`TockError::StorageError`] for other failures.
#[allow(clippy::needless_pass_by_value)]
#[uniffi::export]
pub fn open_workspace(
    path: String,
    password: Vec<u8>,
    secret_key: String,
) -> Result<Arc<Workspace>, TockError> {
    let (_account_id, secret_key) =
        tock_crypto::SecretKey::parse(&secret_key).map_err(|_| TockError::InvalidCredentials)?;
    let vault =
        tock_storage::open(&PathBuf::from(&path), &password, &secret_key).map_err(|e| match e {
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

/// Existing-device pairing state, holding the inviter's ephemeral secret
/// until the onboarding blob is computed.
#[derive(uniffi::Object)]
pub struct PairingInviteSession {
    workspace: Arc<Workspace>,
    secret: Mutex<Option<pairing::PairingSecret>>,
    invite: TockPairingInvite,
}

impl core::fmt::Debug for PairingInviteSession {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PairingInviteSession")
            .field("invite", &self.invite)
            .finish_non_exhaustive()
    }
}

/// New-device pairing state, holding the acceptor's ephemeral secret
/// until the onboarding blob is opened.
#[derive(uniffi::Object)]
pub struct PairingAcceptSession {
    secret: Mutex<Option<pairing::AcceptorSecret>>,
    rendezvous_device_id: [u8; 16],
}

impl core::fmt::Debug for PairingAcceptSession {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PairingAcceptSession")
            .field(
                "rendezvous_device_id",
                &hex_encode(&self.rendezvous_device_id),
            )
            .finish_non_exhaustive()
    }
}

#[allow(
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::significant_drop_tightening
)]
impl Workspace {
    fn with_open_vault<T>(
        &self,
        body: impl FnOnce(&OpenVault) -> Result<T, TockError>,
    ) -> Result<T, TockError> {
        let guard = self.vault.lock().map_err(|_| TockError::InternalError {
            message: "workspace mutex poisoned".into(),
        })?;
        let vault = guard.as_ref().ok_or(TockError::Locked)?;
        body(vault)
    }

    /// Run a closure against the open vault's non-secret header. Used by the
    /// account module to derive signup material without exposing the vault.
    pub(crate) fn with_header<T>(
        &self,
        body: impl FnOnce(&tock_core::vault::VaultHeader) -> Result<T, TockError>,
    ) -> Result<T, TockError> {
        self.with_open_vault(|vault| body(vault.header()))
    }
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

    // ── Sync / pairing ───────────────────────────────────────────────

    /// Read local sync metadata needed by platform transports.
    pub fn sync_device_info(&self) -> Result<TockSyncDeviceInfo, TockError> {
        self.with_open_vault(|vault| {
            let device = vault.local_device();
            Ok(TockSyncDeviceInfo {
                vault_id: vault.header().vault_id.to_string(),
                device_id: hex_encode(&device.device_id),
                verifying_key: hex_encode(&device.signing_key.verifying_key().to_bytes()),
                device_label: sync::device_label(vault)?,
                server_url: sync::server_url(vault)?,
                pull_cursor: sync::pull_cursor(vault)?,
            })
        })
    }

    /// Persist the sync server URL for this vault.
    pub fn sync_set_server_url(&self, url: String) -> Result<(), TockError> {
        self.with_open_vault(|vault| sync::set_server_url(vault, &url).map_err(Into::into))
    }

    /// Persist the local device label used during registration.
    pub fn sync_set_device_label(&self, label: String) -> Result<(), TockError> {
        self.with_open_vault(|vault| sync::set_device_label(vault, &label).map_err(Into::into))
    }

    /// Persist the server pull cursor after a successful pull page.
    pub fn sync_set_pull_cursor(&self, cursor: u64) -> Result<(), TockError> {
        self.with_open_vault(|vault| sync::set_pull_cursor(vault, cursor).map_err(Into::into))
    }

    /// Diff local state and return transport-ready event frames.
    pub fn sync_collect_local_changes(&self) -> Result<Vec<TockSyncEventFrame>, TockError> {
        let events =
            self.with_open_vault(|vault| sync::collect_local_changes(vault).map_err(Into::into))?;
        events
            .into_iter()
            .map(|signed| {
                let payload = wire::encode_batch(std::slice::from_ref(&signed))?;
                Ok(TockSyncEventFrame {
                    event_id: signed.event.id.to_string(),
                    device_id: hex_encode(signed.event.device_id.as_bytes()),
                    lamport: signed.event.lamport,
                    payload,
                })
            })
            .collect()
    }

    /// Decode transport frames and ingest the contained remote events.
    pub fn sync_ingest_event_frames(
        &self,
        frames: Vec<Vec<u8>>,
    ) -> Result<TockSyncIngestSummary, TockError> {
        let mut events = Vec::new();
        for frame in frames {
            events.extend(wire::decode_batch(&frame)?);
        }
        let summary =
            self.with_open_vault(|vault| sync::ingest_events(vault, &events).map_err(Into::into))?;
        Ok(TockSyncIngestSummary {
            applied: u32::try_from(summary.applied).unwrap_or(u32::MAX),
            conflicts: u32::try_from(summary.conflicts).unwrap_or(u32::MAX),
        })
    }

    /// List unresolved sync conflicts for review in the app UI.
    pub fn sync_list_conflicts(&self) -> Result<Vec<TockSyncConflict>, TockError> {
        with_vault!(self, |conn| {
            sync::list_conflicts(conn)
                .map(|conflicts| {
                    conflicts
                        .into_iter()
                        .map(|conflict| TockSyncConflict {
                            id: conflict.id.to_string(),
                            entity_kind: conflict.entity_kind,
                            entity_id: conflict.entity_id.to_string(),
                            detail: conflict.detail,
                        })
                        .collect()
                })
                .map_err(Into::into)
        })
    }

    /// Mark a sync conflict resolved.
    pub fn sync_resolve_conflict(&self, id: String) -> Result<bool, TockError> {
        let conflict_id = uuid::Uuid::parse_str(&id).map_err(|_| TockError::InvalidInput {
            message: format!("invalid conflict id: {id}"),
        })?;
        with_vault!(self, |conn| sync::resolve_conflict(conn, conflict_id)
            .map_err(Into::into))
    }

    /// Start the inviter side of a device-pairing handshake.
    pub fn begin_pairing_invite(
        self: Arc<Self>,
        server_url: String,
    ) -> Result<Arc<PairingInviteSession>, TockError> {
        let (secret, invite) = self.with_open_vault(|vault| {
            pairing::generate_invite(vault.header().vault_id, &server_url).map_err(Into::into)
        })?;
        let created_at =
            invite
                .created_at
                .format(&Rfc3339)
                .map_err(|_| TockError::InternalError {
                    message: "failed to format pairing timestamp".into(),
                })?;
        Ok(Arc::new(PairingInviteSession {
            workspace: self,
            secret: Mutex::new(Some(secret)),
            invite: TockPairingInvite {
                vault_id: invite.vault_id.to_string(),
                server_url: invite.server_url,
                inviter_pubkey: hex_encode(invite.ephemeral_pubkey.as_bytes()),
                inviter_fingerprint: hex_encode(&invite.fingerprint),
                created_at,
            },
        }))
    }

    // ── Tasks ────────────────────────────────────────────────────────

    /// Add a new task.
    pub fn add_task(&self, input: TockNewTask) -> Result<TockTask, TockError> {
        let core_input = input.to_core()?;
        with_vault!(self, |conn| {
            tock_storage::repo::task_repo::insert(
                conn,
                &core_input,
                &tock_core::domain::urgency::UrgencyConfig::default(),
            )
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
            tock_storage::repo::task_repo::update(
                conn,
                sid,
                &core_patch,
                &tock_core::domain::urgency::UrgencyConfig::default(),
            )
            .map(Into::into)
            .map_err(Into::into)
        })
    }

    /// Mark a task as done.
    pub fn complete_task(&self, sid: u32) -> Result<TockTask, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::task_repo::set_status(
                conn,
                sid,
                TaskStatus::Done,
                &tock_core::domain::urgency::UrgencyConfig::default(),
            )
            .map(Into::into)
            .map_err(Into::into)
        })
    }

    /// Mark a task as cancelled.
    pub fn cancel_task(&self, sid: u32) -> Result<TockTask, TockError> {
        with_vault!(self, |conn| {
            tock_storage::repo::task_repo::set_status(
                conn,
                sid,
                TaskStatus::Cancelled,
                &tock_core::domain::urgency::UrgencyConfig::default(),
            )
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

#[allow(clippy::missing_errors_doc, clippy::needless_pass_by_value)]
#[uniffi::export]
impl PairingInviteSession {
    /// Pairing invite details to encode as QR/text on the existing device.
    pub fn invite(&self) -> TockPairingInvite {
        self.invite.clone()
    }

    /// Finalize the inviter half of pairing and produce the encoded blob
    /// to upload to the server for the target device.
    pub fn build_onboarding_blob(
        &self,
        peer_pubkey_hex: String,
        peer_fingerprint_hex: String,
        target_device_id_hex: String,
    ) -> Result<Vec<u8>, TockError> {
        let pairing_secret = self.take_secret()?;
        let peer_pubkey = tock_crypto::keyexchange::PublicKey::from_bytes(parse_hex_array::<32>(
            &peer_pubkey_hex,
            "expected 32-byte peer public key",
        )?);
        let peer_fingerprint =
            parse_hex_array::<8>(&peer_fingerprint_hex, "expected 8-byte peer fingerprint")?;
        let target_device = DeviceId::from_bytes(parse_hex_array::<16>(
            &target_device_id_hex,
            "expected 16-byte target device id",
        )?);
        self.workspace.with_open_vault(|vault| {
            let blob = pairing::compute_onboarding_blob(
                pairing_secret,
                &peer_pubkey,
                &peer_fingerprint,
                vault.vault_key(),
                vault.header().vault_id,
                target_device,
            )?;
            Ok(blob.encode())
        })
    }
}

impl PairingInviteSession {
    fn take_secret(&self) -> Result<pairing::PairingSecret, TockError> {
        let mut guard = self.secret.lock().map_err(|_| TockError::InternalError {
            message: "pairing session mutex poisoned".into(),
        })?;
        guard.take().ok_or_else(|| TockError::InvalidState {
            message: "pairing invite has already been completed".into(),
        })
    }
}

#[allow(clippy::missing_errors_doc, clippy::needless_pass_by_value)]
#[uniffi::export]
impl PairingAcceptSession {
    /// Values that must be relayed to the existing device during pairing.
    pub fn details(&self) -> Result<TockPairingAcceptorInfo, TockError> {
        let (accepter_pubkey, accepter_fingerprint) = {
            let guard = self.secret.lock().map_err(|_| TockError::InternalError {
                message: "pairing session mutex poisoned".into(),
            })?;
            let acceptor = guard.as_ref().ok_or_else(|| TockError::InvalidState {
                message: "pairing accept session has already been used".into(),
            })?;
            let details = (
                hex_encode(acceptor.public_key().as_bytes()),
                hex_encode(&acceptor.fingerprint()),
            );
            drop(guard);
            details
        };
        Ok(TockPairingAcceptorInfo {
            accepter_pubkey,
            accepter_fingerprint,
            rendezvous_device_id: hex_encode(&self.rendezvous_device_id),
        })
    }

    /// Open the onboarding blob, create the paired vault locally, and
    /// return an unlocked workspace ready for sync registration/pull.
    pub fn complete_onboarding(
        &self,
        path: String,
        password: Vec<u8>,
        secret_key: String,
        invite: TockPairingInvite,
        blob: Vec<u8>,
        device_label: Option<String>,
    ) -> Result<Arc<Workspace>, TockError> {
        let acceptor_secret = self.take_secret()?;
        let (account_id, account_secret_key) =
            tock_crypto::SecretKey::parse(&secret_key).map_err(|_| TockError::InvalidInput {
                message: "invalid account Secret Key (Emergency-Kit string)".into(),
            })?;
        let account_id = uuid::Uuid::from_bytes(account_id);
        let vault_id =
            uuid::Uuid::parse_str(&invite.vault_id).map_err(|_| TockError::InvalidInput {
                message: format!("invalid vault id: {}", invite.vault_id),
            })?;
        let inviter_pubkey =
            tock_crypto::keyexchange::PublicKey::from_bytes(parse_hex_array::<32>(
                &invite.inviter_pubkey,
                "expected 32-byte inviter public key",
            )?);
        let inviter_fingerprint = parse_hex_array::<8>(
            &invite.inviter_fingerprint,
            "expected 8-byte inviter fingerprint",
        )?;
        if pairing::fingerprint(&inviter_pubkey) != inviter_fingerprint {
            return Err(TockError::InvalidInput {
                message: "inviter fingerprint mismatch".into(),
            });
        }
        let blob = OnboardingBlob::decode(&blob)?;
        let vault_key =
            pairing::open_onboarding_blob(acceptor_secret, &inviter_pubkey, &blob, vault_id)?;
        let label = device_label.as_deref().or(Some("paired"));
        let vault = tock_storage::vault::init_with_key(
            &PathBuf::from(&path),
            &password,
            &account_secret_key,
            account_id,
            vault_id,
            vault_key,
            label,
        )?;
        sync::set_server_url(&vault, &invite.server_url)?;
        if let Some(label) = &device_label {
            sync::set_device_label(&vault, label)?;
        }
        Ok(Arc::new(Workspace {
            vault: Mutex::new(Some(vault)),
            path,
        }))
    }
}

impl PairingAcceptSession {
    fn take_secret(&self) -> Result<pairing::AcceptorSecret, TockError> {
        let mut guard = self.secret.lock().map_err(|_| TockError::InternalError {
            message: "pairing session mutex poisoned".into(),
        })?;
        guard.take().ok_or_else(|| TockError::InvalidState {
            message: "pairing accept session has already been completed".into(),
        })
    }
}

fn parse_hex_array<const N: usize>(input: &str, message: &str) -> Result<[u8; N], TockError> {
    let bytes = parse_hex_bytes(input)?;
    bytes.try_into().map_err(|_| TockError::InvalidInput {
        message: message.into(),
    })
}

fn parse_hex_bytes(input: &str) -> Result<Vec<u8>, TockError> {
    if !input.len().is_multiple_of(2) {
        return Err(TockError::InvalidInput {
            message: "hex input must have even length".into(),
        });
    }
    let mut out = Vec::with_capacity(input.len() / 2);
    for chunk in input.as_bytes().chunks(2) {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Result<u8, TockError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(TockError::InvalidInput {
            message: "invalid hex character".into(),
        }),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}
