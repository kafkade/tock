//! Basic server metrics endpoint.
//!
//! Exposes `GET /metrics` returning JSON counters. This is a
//! lightweight foundation — production deployments should wire in
//! Prometheus or `OpenTelemetry` for real observability.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::Json;
use axum::response::IntoResponse;
use serde::Serialize;

/// Global atomic counters — cheap, lock-free, and `Send + Sync`.
pub struct Metrics {
    /// Total HTTP requests served.
    pub requests_total: AtomicU64,
    /// Total events stored (push accepted).
    pub events_stored: AtomicU64,
    /// Total encrypted bytes stored (payload sizes).
    pub bytes_stored: AtomicU64,
    /// Total push requests.
    pub pushes_total: AtomicU64,
    /// Total pull requests.
    pub pulls_total: AtomicU64,
    /// Total rate-limited requests (429s).
    pub rate_limited: AtomicU64,
}

impl Metrics {
    /// All counters at zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            pushes_total: AtomicU64::new(0),
            pulls_total: AtomicU64::new(0),
            events_stored: AtomicU64::new(0),
            bytes_stored: AtomicU64::new(0),
            rate_limited: AtomicU64::new(0),
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// JSON snapshot of the current metrics.
#[derive(Serialize)]
pub struct MetricsSnapshot {
    /// Total HTTP requests served.
    pub requests_total: u64,
    /// Total events stored.
    pub events_stored: u64,
    /// Total encrypted bytes stored.
    pub bytes_stored: u64,
    /// Total push requests.
    pub pushes_total: u64,
    /// Total pull requests.
    pub pulls_total: u64,
    /// Total rate-limited requests.
    pub rate_limited: u64,
}

/// `GET /metrics` — return current metric counters.
pub async fn metrics(
    axum::extract::State(state): axum::extract::State<crate::state::AppState>,
) -> impl IntoResponse {
    let m = &state.metrics;
    Json(MetricsSnapshot {
        requests_total: m.requests_total.load(Ordering::Relaxed),
        events_stored: m.events_stored.load(Ordering::Relaxed),
        bytes_stored: m.bytes_stored.load(Ordering::Relaxed),
        pushes_total: m.pushes_total.load(Ordering::Relaxed),
        pulls_total: m.pulls_total.load(Ordering::Relaxed),
        rate_limited: m.rate_limited.load(Ordering::Relaxed),
    })
}
