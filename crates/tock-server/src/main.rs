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
//! - `--mode self-hosted` (default): no accounts, no billing, no quotas.
//! - `--mode hosted`: accounts, subscription tiers, rate limiting,
//!   usage tracking, and metrics.
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
//! - `POST /v1/accounts` — create account (hosted mode only)
//! - `GET /v1/accounts/:id` — account info (hosted mode only)
//! - `GET /v1/accounts/:id/usage` — usage stats (hosted mode only)

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;

use tock_server::ServerMode;

/// tock-server — encrypted blob store for tock sync (AGPL-3.0-only).
#[derive(Parser, Debug)]
#[command(name = "tock-server", version, about)]
struct Args {
    /// Address to bind to.
    #[arg(long, default_value = "0.0.0.0:8080", env = "TOCK_BIND")]
    bind: SocketAddr,

    /// Directory for persistent data (`SQLite` database).
    #[arg(long, default_value = "./data", env = "TOCK_DATA_DIR")]
    data_dir: PathBuf,

    /// Server mode: `self-hosted` (default) or `hosted`.
    #[arg(long, default_value = "self-hosted", env = "TOCK_MODE")]
    mode: String,
}

#[allow(clippy::cognitive_complexity)]
fn main() {
    let args = Args::parse();

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
