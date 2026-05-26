//! Server error types.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error as ThisError;

/// Server errors.
#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// `SQLite` error.
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),

    /// Request authentication failed.
    #[allow(dead_code)]
    #[error("unauthorized: {0}")]
    Unauthorized(&'static str),

    /// Request body or parameters are invalid.
    #[error("bad request: {0}")]
    BadRequest(String),

    /// Requested resource not found.
    #[error("not found")]
    NotFound,

    /// Rate limit exceeded.
    #[allow(dead_code)]
    #[error("rate limit exceeded")]
    RateLimited,

    /// Storage or device quota exceeded.
    #[allow(dead_code)]
    #[error("quota exceeded: {0}")]
    QuotaExceeded(String),

    /// Internal server error.
    #[error("internal: {0}")]
    Internal(String),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::Db(_) | Self::Internal(_) => {
                tracing::error!(error = %self, "internal server error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
            Self::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, (*msg).to_string()),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Self::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            Self::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate limit exceeded".to_string(),
            ),
            Self::QuotaExceeded(msg) => (StatusCode::FORBIDDEN, msg.clone()),
        };
        let body = serde_json::json!({ "error": message });
        (status, axum::Json(body)).into_response()
    }
}
