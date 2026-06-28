//! Shared application state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::auth::PendingHandshake;
use crate::billing::ServerMode;
use crate::db::ServerDb;
use crate::metrics::Metrics;
use crate::quota::RateLimiter;

/// In-memory store of pending SRP handshakes, keyed by a random
/// `handshake_id` minted at `start` and consumed at `finish`. Kept in memory
/// (never persisted) so the server-secret ephemeral `b` lives no longer than
/// the brief window between the two requests.
#[allow(clippy::redundant_pub_crate)]
pub(crate) type PendingHandshakes = Arc<Mutex<HashMap<String, PendingHandshake>>>;

/// Shared server state, cheaply cloneable via `Arc`.
#[derive(Clone)]
pub struct AppState {
    /// Server database.
    pub(crate) db: Arc<ServerDb>,
    /// Operating mode (`self-hosted` or `hosted`).
    pub(crate) mode: ServerMode,
    /// Per-account / per-client rate limiter (registration + hosted requests).
    pub(crate) rate_limiter: Arc<RateLimiter>,
    /// Global metric counters.
    pub(crate) metrics: Arc<Metrics>,
    /// Pending SRP login handshakes awaiting their `finish` request.
    pub(crate) pending: PendingHandshakes,
}
