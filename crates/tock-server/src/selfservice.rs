//! Self-service account endpoints (issue #131).
//!
//! These let a signed-in user manage their own account from the browser:
//! rotate their password, and list/revoke their sessions and devices. Every
//! handler is scoped to the caller's own account via [`authorize_sync`], so a
//! user can only ever see or mutate their own resources.
//!
//! Zero-knowledge holds throughout: password rotation is performed entirely in
//! the client's WASM (re-deriving the URK and re-wrapping the Vault Key), and
//! the server only ever receives the freshly derived, non-secret SRP verifier
//! material and opaque re-wrapped header bytes.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::auth::{authorize_sync, presented_token_hash, unix_now};
use crate::codec::{base64_decode, hex_encode, parse_hex_16};
use crate::error::Error;
use crate::state::AppState;

// ── Password rotation ────────────────────────────────────────────────

/// Request body for `PUT /v1/account/srp-verifier`.
///
/// All fields are client-computed and non-secret. The optional `header` carries
/// the re-wrapped vault header (base64) for accounts that have a real Vault Key;
/// browser-only accounts that never uploaded a header omit it.
#[derive(Deserialize)]
pub struct RotatePasswordRequest {
    /// New random SRP salt (base64).
    pub srp_salt: String,
    /// New SRP verifier `v = g^x mod N` (base64).
    pub srp_verifier: String,
    /// SRP group/hash identifier (e.g. `RFC5054-4096-SHA256`).
    pub srp_group: String,
    /// Opaque KDF parameters (JSON) the client needs to re-derive the URK.
    pub kdf_params: serde_json::Value,
    /// Optional base64 re-wrapped vault header (present when the account owns a
    /// stored header / real Vault Key).
    #[serde(default)]
    pub header: Option<String>,
}

/// `PUT /v1/account/srp-verifier` — rotate the caller's SRP verifier (and, when
/// supplied, the re-wrapped vault header) after a client-side password change.
///
/// The active session is the authorization gate: the browser proved possession
/// of the current password at login, and rotation additionally requires the old
/// password client-side to re-wrap the Vault Key.
pub async fn rotate_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RotatePasswordRequest>,
) -> Result<impl IntoResponse, Error> {
    let auth = authorize_sync(&state, &headers).await?;

    let salt = base64_decode(&body.srp_salt)?;
    let verifier = base64_decode(&body.srp_verifier)?;
    if salt.is_empty() || verifier.is_empty() {
        return Err(Error::BadRequest(
            "srp_salt and srp_verifier are required".into(),
        ));
    }
    let group = body.srp_group;
    let kdf_params = serde_json::to_string(&body.kdf_params)
        .map_err(|e| Error::BadRequest(format!("invalid kdf_params: {e}")))?;
    let header_bytes = match body.header.as_deref() {
        Some(h) if !h.is_empty() => Some(base64_decode(h)?),
        _ => None,
    };

    let db = state.db.clone();
    let account_id = auth.account_id;
    let updated = tokio::task::spawn_blocking(move || {
        let ok = db.update_srp_credentials(&account_id, &salt, &verifier, &group, &kdf_params)?;
        if let Some(header) = header_bytes {
            db.update_vault_header_for_account(&account_id, &header)?;
        }
        Ok::<_, Error>(ok)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    if updated {
        Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
    } else {
        Err(Error::NotFound)
    }
}

// ── Sessions ─────────────────────────────────────────────────────────

/// A live session as rendered by the self-service API.
#[derive(Serialize)]
pub struct SessionItem {
    /// SHA-256 hash of the session bearer token — its stable identifier and the
    /// value passed to the revoke endpoint.
    pub id: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// Absolute expiry (Unix seconds).
    pub expires_at: i64,
    /// Whether this is the session making the request.
    pub current: bool,
}

/// `GET /v1/account/sessions` — list the caller's live sessions.
pub async fn list_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<SessionItem>>, Error> {
    let auth = authorize_sync(&state, &headers).await?;
    let current = presented_token_hash(&headers).ok();
    let now = unix_now();
    let db = state.db.clone();
    let account_id = auth.account_id;
    let sessions = tokio::task::spawn_blocking(move || db.list_sessions(&account_id, now))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;
    Ok(Json(
        sessions
            .into_iter()
            .map(|s| SessionItem {
                current: current.as_deref() == Some(s.token_hash.as_str()),
                id: s.token_hash,
                created_at: s.created_at,
                expires_at: s.expires_at,
            })
            .collect(),
    ))
}

/// `DELETE /v1/account/sessions/{token_hash}` — revoke one of the caller's
/// sessions by its identifier.
pub async fn revoke_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(token_hash): Path<String>,
) -> Result<impl IntoResponse, Error> {
    let auth = authorize_sync(&state, &headers).await?;
    let db = state.db.clone();
    let account_id = auth.account_id;
    let deleted = tokio::task::spawn_blocking(move || db.delete_session(&account_id, &token_hash))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;
    if deleted {
        Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
    } else {
        Err(Error::NotFound)
    }
}

/// `POST /v1/account/sessions/revoke-others` — end every session except the one
/// making the request (e.g. after a password rotation).
pub async fn revoke_other_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, Error> {
    let auth = authorize_sync(&state, &headers).await?;
    let keep = presented_token_hash(&headers)?;
    let db = state.db.clone();
    let account_id = auth.account_id;
    let revoked =
        tokio::task::spawn_blocking(move || db.delete_sessions_except(&account_id, &keep))
            .await
            .map_err(|e| Error::Internal(e.to_string()))??;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({ "revoked": revoked })),
    ))
}

// ── Devices ──────────────────────────────────────────────────────────

/// A registered device as rendered by the self-service API.
#[derive(Serialize)]
pub struct DeviceItem {
    /// Hex-encoded 16-byte device id.
    pub id: String,
    /// Optional human label supplied at registration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// RFC 3339 registration timestamp.
    pub registered_at: String,
    /// Whether the device has been revoked.
    pub revoked: bool,
}

/// `GET /v1/account/devices` — list the devices registered to the caller.
pub async fn list_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DeviceItem>>, Error> {
    let auth = authorize_sync(&state, &headers).await?;
    let db = state.db.clone();
    let account_id = auth.account_id;
    let devices = tokio::task::spawn_blocking(move || db.list_devices_for_account(&account_id))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;
    Ok(Json(
        devices
            .into_iter()
            .map(|d| DeviceItem {
                id: hex_encode(&d.device_id),
                label: d.label,
                registered_at: d.registered_at,
                revoked: d.revoked,
            })
            .collect(),
    ))
}

/// `DELETE /v1/account/devices/{device_id}` — revoke one of the caller's
/// devices so it can no longer sync.
pub async fn revoke_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
) -> Result<impl IntoResponse, Error> {
    let auth = authorize_sync(&state, &headers).await?;
    let did = parse_hex_16(&device_id)?;
    let db = state.db.clone();
    let account_id = auth.account_id;
    let revoked =
        tokio::task::spawn_blocking(move || db.revoke_device_for_account(&account_id, &did))
            .await
            .map_err(|e| Error::Internal(e.to_string()))??;
    if revoked {
        Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
    } else {
        Err(Error::NotFound)
    }
}
