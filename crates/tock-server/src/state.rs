//! Shared application state.

use std::sync::Arc;

use crate::billing::ServerMode;
use crate::db::ServerDb;
use crate::metrics::Metrics;
use crate::quota::RateLimiter;

/// Shared server state, cheaply cloneable via `Arc`.
#[derive(Clone)]
pub struct AppState {
    /// Server database.
    pub(crate) db: Arc<ServerDb>,
    /// Operating mode (`self-hosted` or `hosted`).
    pub(crate) mode: ServerMode,
    /// Per-account rate limiter (only enforced in hosted mode).
    #[allow(dead_code)]
    pub(crate) rate_limiter: Arc<RateLimiter>,
    /// Global metric counters.
    pub(crate) metrics: Arc<Metrics>,
}
