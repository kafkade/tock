//! Offline admin CLI for `tock-server`.
//!
//! These subcommands operate directly on the server's `SQLite` database — no
//! running server or network auth required — so an operator can bootstrap and
//! manage a self-hosted instance from the host shell.

use std::path::Path;

use crate::accounts::RegistrationPolicy;
use crate::db::ServerDb;

/// A parsed offline admin subcommand.
pub enum AdminCommand {
    /// Provision an admin: mint an admin-role invite pinned to `username`.
    CreateAdmin {
        /// Login identifier for the admin to create.
        username: String,
    },
    /// List all accounts (id, username, role, status, `created_at`).
    ListUsers,
    /// Set the instance registration policy.
    ResetRegistration {
        /// The policy to apply.
        policy: RegistrationPolicy,
    },
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Open the server database under `data_dir` and run an [`AdminCommand`].
///
/// # Errors
/// Returns an error if the database cannot be opened/migrated or the command's
/// query fails.
#[allow(clippy::print_literal)]
pub fn run_admin(data_dir: &Path, cmd: AdminCommand) -> Result<(), BoxError> {
    let db = ServerDb::open(&data_dir.join("tock-server.db"))?;
    match cmd {
        AdminCommand::CreateAdmin { username } => {
            let count = db.account_count()?;
            let token = db.create_invite(Some(&username), "admin")?;
            println!("Admin invite created for '{username}'.");
            println!("Setup token: {token}");
            println!();
            println!(
                "Finish setup from a tock client by registering '{username}' \
                 with this invite token; the account is granted the admin role \
                 and an admin API token is returned on success."
            );
            if count == 0 {
                println!(
                    "(Fresh instance: the first account to register becomes admin \
                     automatically, even without this invite.)"
                );
            }
        }
        AdminCommand::ListUsers => {
            let users = db.list_users()?;
            if users.is_empty() {
                println!("No accounts yet.");
            } else {
                println!(
                    "{:<38} {:<28} {:<6} {:<9} {}",
                    "ID", "USERNAME", "ROLE", "STATUS", "CREATED"
                );
                for u in users {
                    println!(
                        "{:<38} {:<28} {:<6} {:<9} {}",
                        u.id, u.username, u.role, u.status, u.created_at
                    );
                }
            }
        }
        AdminCommand::ResetRegistration { policy } => {
            db.set_registration_policy(policy)?;
            println!("Registration policy set to '{}'.", policy.as_str());
        }
    }
    Ok(())
}
