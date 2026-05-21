//! `tock done` — mark tasks as completed.

use tock_core::domain::task::TaskStatus;

/// The target status for `done` command.
#[must_use]
pub const fn done_status() -> TaskStatus {
    TaskStatus::Done
}

/// The target status for `cancel` command.
#[must_use]
pub const fn cancel_status() -> TaskStatus {
    TaskStatus::Cancelled
}
