//! Shared application state.

use std::sync::Arc;

use crate::db::ServerDb;

/// Shared server state, cheaply cloneable via `Arc`.
#[derive(Clone)]
pub struct AppState {
    /// Server database.
    pub db: Arc<ServerDb>,
}
