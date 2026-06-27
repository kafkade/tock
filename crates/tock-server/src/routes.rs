//! Route handlers for the sync API.
//!
//! All data the server handles is opaque — encrypted blobs that it
//! stores and serves without ever decrypting.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::billing::ServerMode;
use crate::codec::{base64_decode, base64_encode, hex_encode, parse_hex_16, parse_hex_32};
use crate::error::Error;
use crate::state::AppState;

// ── Health ───────────────────────────────────────────────────────────

/// `GET /health`
pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

// ── Device registration ──────────────────────────────────────────────

/// Request body for device registration.
#[derive(Deserialize)]
pub struct RegisterDeviceRequest {
    /// Hex-encoded 16-byte device id.
    pub device_id: String,
    /// Hex-encoded 32-byte Ed25519 verifying key.
    pub verifying_key: String,
    /// Optional human label.
    pub label: Option<String>,
}

/// `POST /v1/vaults/:vault_id/devices`
pub async fn register_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(vault_id): Path<String>,
    Json(body): Json<RegisterDeviceRequest>,
) -> Result<impl IntoResponse, Error> {
    let vault_bytes = parse_hex_16(&vault_id)?;
    let device_bytes = parse_hex_16(&body.device_id)?;
    let vk_bytes = parse_hex_32(&body.verifying_key)?;
    let auth_token = hosted_api_token(state.mode, &headers)?;

    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(token) = auth_token.as_deref() {
            let account_id = db
                .account_id_by_api_token(token)?
                .ok_or(Error::Unauthorized("invalid bearer token"))?;
            db.ensure_vault(&vault_bytes)?;
            db.claim_vault_for_account(&vault_bytes, &account_id)?;
        } else {
            db.ensure_vault(&vault_bytes)?;
        }
        db.register_device(
            &vault_bytes,
            &device_bytes,
            &vk_bytes,
            body.label.as_deref(),
        )
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    Ok((StatusCode::CREATED, Json(serde_json::json!({ "ok": true }))))
}

// ── Push events ──────────────────────────────────────────────────────

/// Request body for pushing events.
#[derive(Deserialize)]
pub struct PushEventsRequest {
    /// Array of events, each containing metadata + opaque payload.
    pub events: Vec<PushEventItem>,
}

/// A single event in a push request.
#[derive(Deserialize)]
pub struct PushEventItem {
    /// Hex-encoded 16-byte event id.
    pub event_id: String,
    /// Hex-encoded 16-byte device id.
    pub device_id: String,
    /// Lamport timestamp.
    pub lamport: i64,
    /// Base64-encoded opaque payload (full wire-format event frame).
    pub payload: String,
}

/// Response for push.
#[derive(Serialize)]
pub struct PushResponse {
    /// Number of newly accepted events.
    pub accepted: usize,
    /// Number of duplicate events (already stored).
    pub duplicates: usize,
    /// Server's highest lamport after processing.
    pub server_lamport: i64,
}

/// `POST /v1/vaults/:vault_id/events/push`
pub async fn push_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(vault_id): Path<String>,
    Json(body): Json<PushEventsRequest>,
) -> Result<Json<PushResponse>, Error> {
    let vault_bytes = parse_hex_16(&vault_id)?;
    let auth_token = hosted_api_token(state.mode, &headers)?;

    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || {
        let hosted_account = if let Some(token) = auth_token.as_deref() {
            let account_id = db
                .account_id_by_api_token(token)?
                .ok_or(Error::Unauthorized("invalid bearer token"))?;
            db.require_vault_access(&vault_bytes, &account_id)?;
            Some(account_id)
        } else {
            db.ensure_vault(&vault_bytes)?;
            None
        };
        let mut accepted = 0_usize;
        let mut duplicates = 0_usize;
        let mut accepted_bytes = 0_i64;
        for item in &body.events {
            let eid = parse_hex_16(&item.event_id)?;
            let did = parse_hex_16(&item.device_id)?;
            let payload = base64_decode(&item.payload)?;
            if db.push_event(&eid, &vault_bytes, &did, item.lamport, &payload)? {
                accepted += 1;
                accepted_bytes += i64::try_from(payload.len()).unwrap_or(i64::MAX);
            } else {
                duplicates += 1;
            }
        }
        let server_lamport = db.max_lamport(&vault_bytes)?;
        if let Some(account_id) = hosted_account {
            db.track_usage(
                &account_id,
                accepted_bytes,
                i64::try_from(accepted).unwrap_or(i64::MAX),
            )?;
        }
        Ok::<_, Error>(PushResponse {
            accepted,
            duplicates,
            server_lamport,
        })
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    Ok(Json(result))
}

// ── Pull events ──────────────────────────────────────────────────────

/// Query parameters for pull.
#[derive(Deserialize)]
pub struct PullQuery {
    /// Return events after this opaque cursor position (server rowid).
    /// Defaults to 0 (from the beginning).
    #[serde(default)]
    pub after: i64,
    /// Maximum events to return. Defaults to 256.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

const fn default_limit() -> usize {
    256
}

/// A single event in a pull response.
#[derive(Serialize)]
pub struct PullEventItem {
    /// Hex-encoded event id.
    pub event_id: String,
    /// Hex-encoded device id.
    pub device_id: String,
    /// Lamport timestamp.
    pub lamport: i64,
    /// Base64-encoded opaque payload.
    pub payload: String,
    /// Server receipt timestamp.
    pub created_at: String,
}

/// Response for pull.
#[derive(Serialize)]
pub struct PullResponse {
    /// Events in this batch.
    pub events: Vec<PullEventItem>,
    /// Opaque cursor to pass as `after` on the next pull to resume after
    /// the last event in this batch.
    pub cursor: i64,
    /// Whether there are more events.
    pub more: bool,
}

/// `GET /v1/vaults/:vault_id/events/pull`
pub async fn pull_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(vault_id): Path<String>,
    Query(query): Query<PullQuery>,
) -> Result<Json<PullResponse>, Error> {
    let vault_bytes = parse_hex_16(&vault_id)?;
    let limit = query.limit.min(256);
    let auth_token = hosted_api_token(state.mode, &headers)?;

    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || {
        if let Some(token) = auth_token.as_deref() {
            let account_id = db
                .account_id_by_api_token(token)?
                .ok_or(Error::Unauthorized("invalid bearer token"))?;
            db.require_vault_access(&vault_bytes, &account_id)?;
        }
        // Request limit+1 to detect "more".
        let events = db.pull_events(&vault_bytes, query.after, limit + 1)?;
        let more = events.len() > limit;
        let mut cursor = query.after;
        let items: Vec<PullEventItem> = events
            .into_iter()
            .take(limit)
            .map(|e| {
                cursor = e.rowid;
                PullEventItem {
                    event_id: hex_encode(&e.id),
                    device_id: hex_encode(&e.device_id),
                    lamport: e.lamport,
                    payload: base64_encode(&e.payload),
                    created_at: e.created_at,
                }
            })
            .collect();
        Ok::<_, Error>(PullResponse {
            events: items,
            cursor,
            more,
        })
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    Ok(Json(result))
}

// ── Onboarding blobs ─────────────────────────────────────────────────

/// Request body for putting an onboarding blob.
#[derive(Deserialize)]
pub struct PutOnboardingBlobRequest {
    /// Base64-encoded encrypted blob.
    pub blob: String,
}

/// `PUT /v1/vaults/:vault_id/onboarding/:device_id`
pub async fn put_onboarding_blob(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((vault_id, device_id)): Path<(String, String)>,
    Json(body): Json<PutOnboardingBlobRequest>,
) -> Result<impl IntoResponse, Error> {
    let vault_bytes = parse_hex_16(&vault_id)?;
    let device_bytes = parse_hex_16(&device_id)?;
    let blob = base64_decode(&body.blob)?;
    let auth_token = hosted_api_token(state.mode, &headers)?;

    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(token) = auth_token.as_deref() {
            let account_id = db
                .account_id_by_api_token(token)?
                .ok_or(Error::Unauthorized("invalid bearer token"))?;
            db.require_vault_access(&vault_bytes, &account_id)?;
        } else {
            db.ensure_vault(&vault_bytes)?;
        }
        db.put_onboarding_blob(&vault_bytes, &device_bytes, &blob)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

/// `GET /v1/vaults/:vault_id/onboarding/:device_id`
pub async fn get_onboarding_blob(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((vault_id, device_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, Error> {
    let vault_bytes = parse_hex_16(&vault_id)?;
    let device_bytes = parse_hex_16(&device_id)?;
    let auth_token = hosted_api_token(state.mode, &headers)?;

    let db = state.db.clone();
    let blob = tokio::task::spawn_blocking(move || {
        if let Some(token) = auth_token.as_deref() {
            let account_id = db
                .account_id_by_api_token(token)?
                .ok_or(Error::Unauthorized("invalid bearer token"))?;
            db.require_vault_access(&vault_bytes, &account_id)?;
        }
        db.get_onboarding_blob(&vault_bytes, &device_bytes)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    blob.map_or_else(
        || Err(Error::NotFound),
        |data| Ok(Json(serde_json::json!({ "blob": base64_encode(&data) }))),
    )
}

// ── Helpers ──────────────────────────────────────────────────────────

fn hosted_api_token(mode: ServerMode, headers: &HeaderMap) -> Result<Option<String>, Error> {
    if mode != ServerMode::Hosted {
        return Ok(None);
    }
    let Some(raw) = headers.get(axum::http::header::AUTHORIZATION) else {
        return Err(Error::Unauthorized("missing bearer token"));
    };
    let value = raw
        .to_str()
        .map_err(|_| Error::Unauthorized("invalid authorization header"))?;
    let Some(token) = value.strip_prefix("Bearer ") else {
        return Err(Error::Unauthorized("expected bearer token"));
    };
    if token.is_empty() {
        return Err(Error::Unauthorized("missing bearer token"));
    }
    Ok(Some(token.to_string()))
}
