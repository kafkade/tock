//! Account management routes for the hosted service.
//!
//! These routes are only active when the server is running in
//! `--mode hosted`. Self-hosted mode returns 404 for all account
//! endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::billing::{ServerMode, Tier, UsageSnapshot};
use crate::error::Error;
use crate::state::AppState;

/// Request body for creating an account.
#[derive(Deserialize)]
pub struct CreateAccountRequest {
    /// Account email.
    pub email: String,
    /// Subscription tier (defaults to "free").
    #[serde(default = "default_tier_str")]
    pub tier: String,
}

fn default_tier_str() -> String {
    "free".to_string()
}

/// Response for account creation.
#[derive(Serialize)]
pub struct CreateAccountResponse {
    /// Account id (hex).
    pub account_id: String,
    /// API token for authenticating requests.
    pub api_token: String,
    /// Subscription tier.
    pub tier: String,
}

/// `POST /v1/accounts` — create a new account (hosted mode only).
pub async fn create_account(
    State(state): State<AppState>,
    Json(body): Json<CreateAccountRequest>,
) -> Result<impl IntoResponse, Error> {
    if state.mode != ServerMode::Hosted {
        return Err(Error::NotFound);
    }
    let tier = Tier::from_str_opt(&body.tier)
        .ok_or_else(|| Error::BadRequest(format!("unknown tier: {}", body.tier)))?;

    let db = state.db.clone();
    let email = body.email.clone();
    let result = tokio::task::spawn_blocking(move || db.create_account(&email, tier))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;

    Ok((
        StatusCode::CREATED,
        Json(CreateAccountResponse {
            account_id: result.0,
            api_token: result.1,
            tier: tier.as_str().to_string(),
        }),
    ))
}

/// Account info response.
#[derive(Serialize)]
pub struct AccountInfoResponse {
    /// Account id.
    pub account_id: String,
    /// Email.
    pub email: String,
    /// Subscription tier.
    pub tier: String,
    /// Current usage snapshot.
    pub usage: UsageSnapshot,
}

/// `GET /v1/accounts/:account_id` — get account info + usage.
pub async fn get_account(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<AccountInfoResponse>, Error> {
    if state.mode != ServerMode::Hosted {
        return Err(Error::NotFound);
    }
    let db = state.db.clone();
    let aid = account_id.clone();
    let info = tokio::task::spawn_blocking(move || db.get_account(&aid))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??
        .ok_or(Error::NotFound)?;

    Ok(Json(AccountInfoResponse {
        account_id,
        email: info.0,
        tier: info.1,
        usage: info.2,
    }))
}

/// `GET /v1/accounts/:account_id/usage` — usage-only endpoint.
pub async fn get_usage(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<UsageSnapshot>, Error> {
    if state.mode != ServerMode::Hosted {
        return Err(Error::NotFound);
    }
    let db = state.db.clone();
    let usage = tokio::task::spawn_blocking(move || db.get_usage(&account_id))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??
        .ok_or(Error::NotFound)?;

    Ok(Json(usage))
}
