//! Offline admin CLI for `tock-server`.
//!
//! These subcommands operate directly on the server's `SQLite` database — no
//! running server or network auth required — so an operator can bootstrap and
//! manage a self-hosted instance from the host shell.

use std::path::{Path, PathBuf};

use crate::accounts::RegistrationPolicy;
use crate::codec::{base64_encode, hex_encode};
use crate::db::ServerDb;
use crate::retention;
use crate::routes::{ArchiveEvent, VaultArchive};

/// Which vaults an offline `export` covers.
pub enum ExportScope {
    /// Every vault on the instance.
    All,
    /// Only the vaults owned by a single account id.
    Account(String),
}

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
    /// Export per-vault ciphertext archives (never decrypting) to `out_dir`.
    Export {
        /// Which vaults to export.
        scope: ExportScope,
        /// Directory the archives are written to (created on demand).
        out_dir: PathBuf,
    },
    /// Take a one-off retained snapshot of the whole database now.
    Snapshot {
        /// Directory the snapshot is written to (created on demand).
        out_dir: PathBuf,
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
        AdminCommand::Export { scope, out_dir } => run_export(&db, scope, &out_dir)?,
        AdminCommand::Snapshot { out_dir } => {
            std::fs::create_dir_all(&out_dir)?;
            let path = out_dir.join(retention::snapshot_filename(time::OffsetDateTime::now_utc()));
            db.snapshot_to(&path)?;
            println!("Wrote snapshot: {}", path.display());
        }
    }
    Ok(())
}

/// Resolve the vaults in scope and write one ciphertext archive per vault. The
/// archives contain only stored ciphertext (vault header + opaque event
/// payloads) — the server never decrypts.
#[allow(clippy::print_literal)]
fn run_export(db: &ServerDb, scope: ExportScope, out_dir: &Path) -> Result<(), BoxError> {
    std::fs::create_dir_all(out_dir)?;

    // (vault bytes, label used in the filename).
    let targets: Vec<([u8; 16], String)> = match scope {
        ExportScope::All => db
            .list_vaults()?
            .into_iter()
            .map(|(id, account)| {
                let mut arr = [0_u8; 16];
                let n = id.len().min(16);
                arr[..n].copy_from_slice(&id[..n]);
                let label = account.unwrap_or_else(|| "unowned".to_string());
                (arr, label)
            })
            .collect(),
        ExportScope::Account(account_id) => db
            .vaults_for_account(&account_id)?
            .into_iter()
            .map(|id| (id, account_id.clone()))
            .collect(),
    };

    if targets.is_empty() {
        println!("No vaults matched; nothing to export.");
        return Ok(());
    }

    for (vault, label) in targets {
        let archive = build_archive(db, &vault)?;
        let file = out_dir.join(format!("{label}-{}.json", hex_encode(&vault)));
        let json = serde_json::to_string_pretty(&archive)?;
        std::fs::write(&file, json)?;
        println!(
            "Exported vault {} ({} events) -> {}",
            hex_encode(&vault),
            archive.events.len(),
            file.display()
        );
    }
    Ok(())
}

/// Build the ciphertext archive for one vault (header + full event log).
fn build_archive(db: &ServerDb, vault: &[u8; 16]) -> Result<VaultArchive, BoxError> {
    let header = db.get_vault_header(vault)?.map(|h| base64_encode(&h));
    let events = db
        .all_events(vault)?
        .into_iter()
        .map(|e| ArchiveEvent {
            event_id: hex_encode(&e.id),
            device_id: hex_encode(&e.device_id),
            lamport: e.lamport,
            payload: base64_encode(&e.payload),
        })
        .collect();
    Ok(VaultArchive {
        vault_id: hex_encode(vault),
        header,
        events,
    })
}
