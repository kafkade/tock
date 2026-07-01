//! # tock-server
//!
//! Optional sync server for tock. Licensed under **AGPL-3.0-only** — see
//! `crates/tock-server/LICENSE`.
//!
//! The server is an encrypted blob store: it never sees plaintext user
//! data. See `docs/architecture.md` §6 and ADR-006 for the licensing
//! rationale.
//!
//! ## Modes
//!
//! - `--mode self-hosted` (default): first-class accounts (admin/user) with
//!   SRP-verifier storage and a configurable registration policy; no billing,
//!   no quotas.
//! - `--mode hosted`: everything in self-hosted, plus billing accounts,
//!   subscription tiers, usage tracking, and metrics.
//!
//! ## Routes
//!
//! - `GET /health` — health check
//! - `GET /metrics` — server metrics (JSON counters)
//! - `POST /v1/vaults/:vault_id/devices` — register a device
//! - `POST /v1/vaults/:vault_id/events/push` — push encrypted events
//! - `GET /v1/vaults/:vault_id/events/pull` — pull events by cursor
//! - `PUT /v1/vaults/:vault_id/onboarding/:device_id` — store pairing blob
//! - `GET /v1/vaults/:vault_id/onboarding/:device_id` — retrieve pairing blob
//! - `POST /v1/accounts/register` — self-hosted account registration (SRP)
//! - `GET|POST /v1/admin/users` — list users / mint invite (admin)
//! - `DELETE /v1/admin/users/:id`, `POST …/disable`, `…/enable` — manage users
//! - `GET|PUT /v1/admin/settings` — read/set registration policy (admin)
//! - `POST /v1/accounts` — create billing account (hosted mode only)
//! - `GET /v1/accounts/:id` — account info (hosted mode only)
//! - `GET /v1/accounts/:id/usage` — usage stats (hosted mode only)
//!
//! ## Offline admin CLI
//!
//! - `tock-server admin create-admin --username <u>`
//! - `tock-server admin list-users`
//! - `tock-server admin reset-registration --policy <open|invite-only|disabled>`
//!
//! ## Environment bootstrap
//!
//! - `TOCK_REGISTRATION_POLICY=<open|invite-only|disabled>` — pin the policy on
//!   startup.
//! - `TOCK_ADMIN_USERNAME=<u>` — on a fresh instance, mint an admin invite for
//!   `<u>` and log the setup token (skipped once an admin exists).

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use tock_server::{AdminCommand, RegistrationPolicy, ServerMode};

/// tock-server — encrypted blob store for tock sync (AGPL-3.0-only).
#[derive(Parser, Debug)]
#[command(name = "tock-server", version, about)]
struct Args {
    /// Address to bind to.
    #[arg(long, default_value = "0.0.0.0:8080", env = "TOCK_BIND")]
    bind: SocketAddr,

    /// Directory for persistent data (`SQLite` database).
    #[arg(long, default_value = "./data", env = "TOCK_DATA_DIR", global = true)]
    data_dir: PathBuf,

    /// Server mode: `self-hosted` (default) or `hosted`.
    #[arg(long, default_value = "self-hosted", env = "TOCK_MODE")]
    mode: String,

    /// Optional subcommand. With none, the server starts and serves requests.
    #[command(subcommand)]
    command: Option<Command>,
}

/// Top-level subcommands.
#[derive(Subcommand, Debug)]
enum Command {
    /// Offline admin operations on the server database (no running server).
    Admin {
        /// The admin action to perform.
        #[command(subcommand)]
        action: AdminAction,
    },
}

/// Offline admin actions.
#[derive(Subcommand, Debug)]
enum AdminAction {
    /// Provision an admin by minting an admin-role invite.
    CreateAdmin {
        /// Login identifier (email or username) for the admin.
        #[arg(long)]
        username: String,
    },
    /// List all accounts.
    ListUsers,
    /// Set the registration policy: `open`, `invite-only`, or `disabled`.
    ResetRegistration {
        /// The policy to apply.
        #[arg(long)]
        policy: String,
    },
}

fn run_admin_command(data_dir: &std::path::Path, action: AdminAction) -> ! {
    let cmd = match action {
        AdminAction::CreateAdmin { username } => AdminCommand::CreateAdmin { username },
        AdminAction::ListUsers => AdminCommand::ListUsers,
        AdminAction::ResetRegistration { policy } => {
            let Some(parsed) = RegistrationPolicy::from_str_opt(policy.trim()) else {
                eprintln!(
                    "error: unknown policy '{policy}', expected open | invite-only | disabled"
                );
                std::process::exit(1);
            };
            AdminCommand::ResetRegistration { policy: parsed }
        }
    };
    match tock_server::run_admin(data_dir, cmd) {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

#[allow(clippy::cognitive_complexity)]
fn main() {
    let args = Args::parse();

    if let Some(Command::Admin { action }) = args.command {
        run_admin_command(&args.data_dir, action);
    }

    let mode = ServerMode::from_str_opt(&args.mode).unwrap_or_else(|| {
        eprintln!(
            "error: unknown mode '{}', expected 'self-hosted' or 'hosted'",
            args.mode
        );
        std::process::exit(1);
    });

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        license = "AGPL-3.0-only",
        bind = %args.bind,
        data_dir = %args.data_dir.display(),
        mode = %mode,
        "starting tock-server"
    );

    if let Err(e) = std::fs::create_dir_all(&args.data_dir) {
        tracing::error!(error = %e, "failed to create data directory");
        std::process::exit(1);
    }

    let app_state = match tock_server::open_app_state(&args.data_dir, mode) {
        Ok(state) => state,
        Err(e) => {
            tracing::error!(error = %e, "failed to open database");
            std::process::exit(1);
        }
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| {
            tracing::error!(error = %e, "failed to build tokio runtime");
            std::process::exit(1);
        });

    rt.block_on(async {
        let listener = match tokio::net::TcpListener::bind(args.bind).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(error = %e, "failed to bind");
                std::process::exit(1);
            }
        };
        tracing::info!(addr = %args.bind, "listening");
        if let Err(e) = tock_server::serve(listener, app_state).await {
            tracing::error!(error = %e, "server error");
        }
    });
}
