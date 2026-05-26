//! # tock-server
//!
//! Optional sync server for tock. Licensed under **AGPL-3.0-only** — see
//! `crates/tock-server/LICENSE`.
//!
//! The server is an encrypted blob store: it never sees plaintext user
//! data. See `docs/architecture.md` §6 and ADR-006 for the licensing
//! rationale.
//!
//! ## Routes
//!
//! - `GET /health` — health check
//! - `POST /v1/vaults/:vault_id/devices` — register a device
//! - `POST /v1/vaults/:vault_id/events/push` — push encrypted events
//! - `GET /v1/vaults/:vault_id/events/pull` — pull events by cursor
//! - `PUT /v1/vaults/:vault_id/onboarding/:device_id` — store pairing blob
//! - `GET /v1/vaults/:vault_id/onboarding/:device_id` — retrieve pairing blob

mod db;
mod error;
mod routes;
mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post, put};
use clap::Parser;
use tower_http::trace::TraceLayer;

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
}

fn main() {
    let args = Args::parse();

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
        "starting tock-server"
    );

    // Ensure data directory exists.
    if let Err(e) = std::fs::create_dir_all(&args.data_dir) {
        tracing::error!(error = %e, "failed to create data directory");
        std::process::exit(1);
    }

    let db_path = args.data_dir.join("tock-server.db");
    let server_db = match db::ServerDb::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            tracing::error!(error = %e, "failed to open database");
            std::process::exit(1);
        }
    };

    let state = state::AppState {
        db: Arc::new(server_db),
    };

    let app = Router::new()
        .route("/health", get(routes::health))
        .route(
            "/v1/vaults/{vault_id}/devices",
            post(routes::register_device),
        )
        .route(
            "/v1/vaults/{vault_id}/events/push",
            post(routes::push_events),
        )
        .route(
            "/v1/vaults/{vault_id}/events/pull",
            get(routes::pull_events),
        )
        .route(
            "/v1/vaults/{vault_id}/onboarding/{device_id}",
            put(routes::put_onboarding_blob).get(routes::get_onboarding_blob),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state);

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
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(error = %e, "server error");
        }
    });
}
