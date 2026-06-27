//! Admin API: user management and server settings.
//!
//! These endpoints let an admin list users, mint invites, disable/enable and
//! delete accounts, and read/set the registration policy. They are available in
//! every server mode (accounts are a first-class self-hosted feature).
//!
//! ## Authorization seam
//!
//! Every handler requires an **admin bearer token** ([`require_admin`]) — the
//! interim token minted when an admin account is provisioned. Issue #130
//! introduces SRP login + session tokens and will extend this guard to accept
//! them; the endpoint signatures and behavior do not change. The server never
//! gains the ability to read any user's plaintext — zero-knowledge holds for
//! admins too (ADR-011 §5).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::accounts::RegistrationPolicy;
use crate::error::Error;
use crate::state::AppState;

/// Authorize the request as an active admin, returning the admin account id.
///
/// Expects `Authorization: Bearer <admin-token>`. Issue #130 extends this to
/// also accept SRP session tokens.
async fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<String, Error> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)
        .ok_or(Error::Unauthorized("missing bearer token"))?;
    let value = raw
        .to_str()
        .map_err(|_| Error::Unauthorized("invalid authorization header"))?;
    let token = value
        .strip_prefix("Bearer ")
        .filter(|t| !t.is_empty())
        .ok_or(Error::Unauthorized("expected bearer token"))?
        .to_string();

    let db = state.db.clone();
    let account_id = tokio::task::spawn_blocking(move || db.admin_account_id_by_token(&token))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??
        .ok_or(Error::Forbidden("admin privileges required"))?;
    Ok(account_id)
}

/// A user as rendered by the admin API.
#[derive(Serialize)]
pub struct AdminUser {
    /// Account id.
    pub id: String,
    /// Login identifier.
    pub username: String,
    /// Role (`admin` or `user`).
    pub role: String,
    /// Status (`active` or `disabled`).
    pub status: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// `GET /v1/admin/users` — list all accounts.
pub async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AdminUser>>, Error> {
    require_admin(&state, &headers).await?;
    let db = state.db.clone();
    let users = tokio::task::spawn_blocking(move || db.list_users())
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;
    Ok(Json(
        users
            .into_iter()
            .map(|u| AdminUser {
                id: u.id,
                username: u.username,
                role: u.role,
                status: u.status,
                created_at: u.created_at,
            })
            .collect(),
    ))
}

/// Request body for minting an invite.
#[derive(Deserialize)]
pub struct CreateInviteRequest {
    /// Optional username the invite is pinned to.
    #[serde(default)]
    pub username: Option<String>,
    /// Role granted on registration (`user` or `admin`); defaults to `user`.
    #[serde(default)]
    pub role: Option<String>,
}

/// Response carrying a freshly minted invite token.
#[derive(Serialize)]
pub struct CreateInviteResponse {
    /// The opaque invite token to hand to the invitee.
    pub invite_token: String,
    /// Role the invite grants.
    pub role: String,
    /// Username the invite is pinned to, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

/// `POST /v1/admin/users` — mint an invite for a new account.
///
/// Admins cannot set passwords (zero-knowledge), so "creating a user" issues an
/// invite the user redeems via `POST /v1/accounts/register` with their own
/// client-computed SRP credentials.
pub async fn create_user_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateInviteRequest>,
) -> Result<impl IntoResponse, Error> {
    require_admin(&state, &headers).await?;

    let role = match body.role.as_deref() {
        None | Some("user") => "user".to_string(),
        Some("admin") => "admin".to_string(),
        Some(other) => return Err(Error::BadRequest(format!("unknown role: {other}"))),
    };
    let username = body.username.filter(|u| !u.trim().is_empty());

    let db = state.db.clone();
    let pinned = username.clone();
    let role_for_db = role.clone();
    let token =
        tokio::task::spawn_blocking(move || db.create_invite(pinned.as_deref(), &role_for_db))
            .await
            .map_err(|e| Error::Internal(e.to_string()))??;

    Ok((
        StatusCode::CREATED,
        Json(CreateInviteResponse {
            invite_token: token,
            role,
            username,
        }),
    ))
}

/// `POST /v1/admin/users/:id/disable` — disable an account.
pub async fn disable_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(account_id): Path<String>,
) -> Result<impl IntoResponse, Error> {
    set_status(&state, &headers, &account_id, "disabled").await
}

/// `POST /v1/admin/users/:id/enable` — re-enable an account.
pub async fn enable_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(account_id): Path<String>,
) -> Result<impl IntoResponse, Error> {
    set_status(&state, &headers, &account_id, "active").await
}

async fn set_status(
    state: &AppState,
    headers: &HeaderMap,
    account_id: &str,
    status: &'static str,
) -> Result<(StatusCode, Json<serde_json::Value>), Error> {
    require_admin(state, headers).await?;
    let db = state.db.clone();
    let id = account_id.to_string();
    let updated = tokio::task::spawn_blocking(move || db.set_user_status(&id, status))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;
    if updated {
        Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
    } else {
        Err(Error::NotFound)
    }
}

/// `DELETE /v1/admin/users/:id` — delete an account.
pub async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(account_id): Path<String>,
) -> Result<impl IntoResponse, Error> {
    require_admin(&state, &headers).await?;
    let db = state.db.clone();
    let deleted = tokio::task::spawn_blocking(move || db.delete_user(&account_id))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;
    if deleted {
        Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
    } else {
        Err(Error::NotFound)
    }
}

/// Registration-policy settings payload (GET response / PUT request).
#[derive(Serialize, Deserialize)]
pub struct RegistrationSettings {
    /// One of `open`, `invite-only`, `disabled`.
    pub registration_policy: String,
}

/// `GET /v1/admin/settings` — read server settings.
pub async fn get_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RegistrationSettings>, Error> {
    require_admin(&state, &headers).await?;
    let db = state.db.clone();
    let policy = tokio::task::spawn_blocking(move || db.registration_policy())
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;
    Ok(Json(RegistrationSettings {
        registration_policy: policy.as_str().to_string(),
    }))
}

/// `PUT /v1/admin/settings` — update the registration policy.
pub async fn put_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegistrationSettings>,
) -> Result<impl IntoResponse, Error> {
    require_admin(&state, &headers).await?;
    let policy = RegistrationPolicy::from_str_opt(&body.registration_policy).ok_or_else(|| {
        Error::BadRequest(format!(
            "unknown registration policy: {}",
            body.registration_policy
        ))
    })?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.set_registration_policy(policy))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;
    Ok((
        StatusCode::OK,
        Json(RegistrationSettings {
            registration_policy: policy.as_str().to_string(),
        }),
    ))
}
