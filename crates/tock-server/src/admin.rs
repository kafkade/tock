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
/// Accepts either the interim admin **API token** (minted when an admin account
/// is provisioned, issue #127) or an **SRP session token** (issue #130) whose
/// account is an active admin. The endpoint signatures and behavior are
/// unchanged.
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

    // 1) Interim admin API token (issue #127).
    let db = state.db.clone();
    let token_for_db = token.clone();
    if let Some(account_id) =
        tokio::task::spawn_blocking(move || db.admin_account_id_by_token(&token_for_db))
            .await
            .map_err(|e| Error::Internal(e.to_string()))??
    {
        return Ok(account_id);
    }

    // 2) SRP session token belonging to an active admin account (issue #130).
    if let Some(account_id) = crate::auth::admin_via_session(state, headers).await? {
        return Ok(account_id);
    }

    Err(Error::Forbidden("admin privileges required"))
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

/// Server settings payload returned by `GET /v1/admin/settings` (and echoed by
/// `PUT`). Carries the registration policy and the optional public server
/// address used by clients and the setup wizard (issue #131).
#[derive(Serialize, Deserialize)]
pub struct InstanceSettings {
    /// One of `open`, `invite-only`, `disabled`.
    pub registration_policy: String,
    /// Public base URL clients should use to reach this instance, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_address: Option<String>,
}

/// Partial update body for `PUT /v1/admin/settings` — every field is optional so
/// the setup wizard and console can patch policy and address independently.
#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    /// New registration policy, when changing it.
    #[serde(default)]
    pub registration_policy: Option<String>,
    /// New public address; an empty string clears it.
    #[serde(default)]
    pub public_address: Option<String>,
}

/// `GET /v1/admin/settings` — read server settings.
pub async fn get_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<InstanceSettings>, Error> {
    require_admin(&state, &headers).await?;
    let db = state.db.clone();
    let settings = tokio::task::spawn_blocking(move || {
        let policy = db.registration_policy()?;
        let address = db.public_address()?;
        Ok::<_, Error>(InstanceSettings {
            registration_policy: policy.as_str().to_string(),
            public_address: address,
        })
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;
    Ok(Json(settings))
}

/// `PUT /v1/admin/settings` — update the registration policy and/or public
/// address. Returns the full, current settings.
pub async fn put_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UpdateSettingsRequest>,
) -> Result<impl IntoResponse, Error> {
    require_admin(&state, &headers).await?;
    let policy = match body.registration_policy.as_deref() {
        None => None,
        Some(raw) => Some(
            RegistrationPolicy::from_str_opt(raw)
                .ok_or_else(|| Error::BadRequest(format!("unknown registration policy: {raw}")))?,
        ),
    };
    let address = body.public_address.map(|a| a.trim().to_string());
    let db = state.db.clone();
    let settings = tokio::task::spawn_blocking(move || {
        if let Some(policy) = policy {
            db.set_registration_policy(policy)?;
        }
        if let Some(address) = address {
            db.set_public_address(&address)?;
        }
        let policy = db.registration_policy()?;
        let public_address = db.public_address()?;
        Ok::<_, Error>(InstanceSettings {
            registration_policy: policy.as_str().to_string(),
            public_address,
        })
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;
    Ok((StatusCode::OK, Json(settings)))
}

/// Aggregate instance statistics returned by `GET /v1/admin/stats` (issue
/// #131). Every value is a non-secret count or byte total over opaque data.
#[derive(Serialize)]
pub struct InstanceStatsResponse {
    /// Total accounts.
    pub accounts_total: i64,
    /// Accounts with the `admin` role.
    pub accounts_admin: i64,
    /// Active accounts.
    pub accounts_active: i64,
    /// Disabled accounts.
    pub accounts_disabled: i64,
    /// Total vaults.
    pub vaults: i64,
    /// Non-revoked registered devices.
    pub devices: i64,
    /// Total stored events.
    pub events: i64,
    /// Total bytes of stored (encrypted) event payloads.
    pub storage_bytes: i64,
}

/// `GET /v1/admin/stats` — instance usage/health counters for the console.
pub async fn get_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<InstanceStatsResponse>, Error> {
    require_admin(&state, &headers).await?;
    let db = state.db.clone();
    let counters = tokio::task::spawn_blocking(move || db.instance_stats())
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;
    Ok(Json(InstanceStatsResponse {
        accounts_total: counters.accounts_total,
        accounts_admin: counters.accounts_admin,
        accounts_active: counters.accounts_active,
        accounts_disabled: counters.accounts_disabled,
        vaults: counters.vaults,
        devices: counters.devices,
        events: counters.events,
        storage_bytes: counters.storage_bytes,
    }))
}
