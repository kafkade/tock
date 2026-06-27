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
//! - `POST /v1/accounts/register` — self-hosted account registration (SRP)
//! - `GET|POST /v1/admin/users` — list users / mint invite (admin)
//! - `DELETE /v1/admin/users/:id`, `POST …/disable`, `…/enable` — manage users
//! - `GET|PUT /v1/admin/settings` — read/set registration policy (admin)
//! - `POST /v1/accounts` — create billing account (hosted mode only)
//! - `GET /v1/accounts/:id` — account info (hosted mode only)
//! - `GET /v1/accounts/:id/usage` — usage stats (hosted mode only)

mod accounts;
mod admin;
mod billing;
mod cli;
mod codec;
mod db;
mod error;
mod metrics;
mod quota;
mod routes;
mod state;

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, post, put};
use tower_http::trace::TraceLayer;

pub use accounts::RegistrationPolicy;
pub use billing::ServerMode;
pub use cli::{AdminCommand, run_admin};
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

    // Seed the registration policy from the environment when set, so operators
    // can pin it declaratively (e.g. in a container). An admin can still change
    // it at runtime via the admin API; the env value only applies on startup.
    if let Ok(raw) = std::env::var("TOCK_REGISTRATION_POLICY") {
        match RegistrationPolicy::from_str_opt(raw.trim()) {
            Some(policy) => {
                db.set_registration_policy(policy)?;
                tracing::info!(policy = %policy, "registration policy set from TOCK_REGISTRATION_POLICY");
            }
            None => {
                tracing::warn!(
                    value = %raw,
                    "ignoring TOCK_REGISTRATION_POLICY: expected open | invite-only | disabled"
                );
            }
        }
    }

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
        )
        // Self-hosted account system (ADR-011 / issue #127) — available in
        // every mode. Registration stores SRP verifiers only.
        .route("/v1/accounts/register", post(accounts::register))
        .route(
            "/v1/admin/users",
            get(admin::list_users).post(admin::create_user_invite),
        )
        .route("/v1/admin/users/{account_id}", delete(admin::delete_user))
        .route(
            "/v1/admin/users/{account_id}/disable",
            post(admin::disable_user),
        )
        .route(
            "/v1/admin/users/{account_id}/enable",
            post(admin::enable_user),
        )
        .route(
            "/v1/admin/settings",
            get(admin::get_settings).put(admin::put_settings),
        );

    // Hosted-mode-only billing routes.
    if state.mode == ServerMode::Hosted {
        app = app
            .route("/v1/accounts", post(accounts::create_account))
            .route("/v1/accounts/{account_id}", get(accounts::get_account))
            .route("/v1/accounts/{account_id}/usage", get(accounts::get_usage));
        tracing::info!("hosted mode: account billing routes enabled");
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
    // `into_make_service_with_connect_info` exposes the peer `SocketAddr` to
    // handlers (used by registration rate limiting). Handlers extract it as
    // `Option<ConnectInfo<SocketAddr>>`, so router-only callers (e.g. tower
    // `oneshot` tests) still work without connect info.
    axum::serve(
        listener,
        build_router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
}
