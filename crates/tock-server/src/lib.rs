//! # tock-server (library)
//!
//! Optional sync server for tock. Licensed under **AGPL-3.0-only** — see
//! `crates/tock-server/LICENSE`.
//!
//! The server is an encrypted blob store: it never sees plaintext user
//! data. See `docs/architecture.md` §6 and ADR-006 for the licensing
//! rationale.
//!
//! This library exposes the router construction ([`build_router`]) and
//! shared state ([`AppState`]) so both the `tock-server` binary and
//! integration tests (e.g. the end-to-end multi-device sync acceptance
//! test) can build an identical server instance.
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

mod accounts;
mod billing;
mod db;
mod error;
mod metrics;
mod quota;
mod routes;
mod state;

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post, put};
use tower_http::trace::TraceLayer;

pub use billing::ServerMode;
pub use state::AppState;

use db::ServerDb;
use metrics::Metrics;
use quota::RateLimiter;

/// Open the server database under `data_dir` and assemble an [`AppState`]
/// for the given [`ServerMode`].
///
/// Shared by the `tock-server` binary and integration tests so both build
/// state the same way.
///
/// # Errors
/// Returns an error if the on-disk database cannot be opened or migrated.
pub fn open_app_state(
    data_dir: &Path,
    mode: ServerMode,
) -> Result<AppState, Box<dyn std::error::Error + Send + Sync>> {
    let db = ServerDb::open(&data_dir.join("tock-server.db"))?;
    Ok(AppState {
        db: Arc::new(db),
        mode,
        rate_limiter: Arc::new(RateLimiter::new()),
        metrics: Arc::new(Metrics::new()),
    })
}

/// Build the server [`Router`] for the given [`AppState`].
///
/// Self-hosted mode exposes only the encrypted blob-store routes;
/// hosted mode additionally enables account management and billing.
/// The binary and integration tests share this single definition so the
/// surface they exercise is identical.
pub fn build_router(state: AppState) -> Router {
    let mut app = Router::new()
        .route("/health", get(routes::health))
        .route("/metrics", get(metrics::metrics))
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
        );

    // Hosted-mode-only routes.
    if state.mode == ServerMode::Hosted {
        app = app
            .route("/v1/accounts", post(accounts::create_account))
            .route("/v1/accounts/{account_id}", get(accounts::get_account))
            .route("/v1/accounts/{account_id}/usage", get(accounts::get_usage));
        tracing::info!("hosted mode: account management and billing routes enabled");
    }

    app.layer(TraceLayer::new_for_http()).with_state(state)
}

/// Serve the router on an already-bound [`tokio::net::TcpListener`].
///
/// This is the async serve loop shared by the binary's `main` and
/// integration tests that need to bind an ephemeral port and drive the
/// server in-process.
///
/// # Errors
/// Returns any I/O error surfaced by the underlying `axum::serve` loop.
pub async fn serve(listener: tokio::net::TcpListener, state: AppState) -> std::io::Result<()> {
    axum::serve(listener, build_router(state)).await
}
