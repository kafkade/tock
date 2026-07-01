//! Route handlers for the sync API.
//!
//! All data the server handles is opaque — encrypted blobs that it
//! stores and serves without ever decrypting.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::auth::{authorize_sync, verify_channel_binding};
use crate::billing::ServerMode;
use crate::codec::{base64_decode, base64_encode, hex_encode, parse_hex_16, parse_hex_32};
use crate::error::Error;
use crate::state::AppState;

// ── Health ───────────────────────────────────────────────────────────

/// `GET /health`
pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

// ── Server info ──────────────────────────────────────────────────────

/// Public instance metadata returned by `GET /v1/server/info`.
///
/// This is unauthenticated on purpose: the web console fetches it before any
/// account exists to decide whether to show the first-run setup wizard. It
/// exposes only non-secret operational facts.
#[derive(Serialize)]
pub struct ServerInfo {
    /// `true` when no account exists yet, so the first registrant will be
    /// bootstrapped as the admin (the browser should show the setup wizard).
    pub setup_required: bool,
    /// Current registration policy (`open`, `invite-only`, `disabled`).
    pub registration_policy: String,
    /// Operating mode (`self-hosted` or `hosted`).
    pub mode: String,
    /// Server crate version.
    pub version: String,
}

/// `GET /v1/server/info` — public instance metadata for first-run gating.
pub async fn server_info(State(state): State<AppState>) -> Result<Json<ServerInfo>, Error> {
    let db = state.db.clone();
    let (count, policy) = tokio::task::spawn_blocking(move || {
        let count = db.account_count()?;
        let policy = db.registration_policy()?;
        Ok::<_, Error>((count, policy))
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    Ok(Json(ServerInfo {
        setup_required: count == 0,
        registration_policy: policy.as_str().to_string(),
        mode: state.mode.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }))
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
    let auth = authorize_sync(&state, &headers).await?;

    let db = state.db.clone();
    let account_id = auth.account_id;
    tokio::task::spawn_blocking(move || {
        db.ensure_vault(&vault_bytes)?;
        db.claim_vault_for_account(&vault_bytes, &account_id)?;
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
    let auth = authorize_sync(&state, &headers).await?;
    verify_channel_binding(&auth, &headers)?;
    let hosted = state.mode == ServerMode::Hosted;
    let account_id = auth.account_id;

    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || {
        db.ensure_vault(&vault_bytes)?;
        db.claim_vault_for_account(&vault_bytes, &account_id)?;
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
        if hosted {
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
    let auth = authorize_sync(&state, &headers).await?;
    verify_channel_binding(&auth, &headers)?;
    let account_id = auth.account_id;

    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || {
        db.require_vault_access(&vault_bytes, &account_id)?;
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
    let auth = authorize_sync(&state, &headers).await?;
    let account_id = auth.account_id;

    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.ensure_vault(&vault_bytes)?;
        db.claim_vault_for_account(&vault_bytes, &account_id)?;
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
    let auth = authorize_sync(&state, &headers).await?;
    let account_id = auth.account_id;

    let db = state.db.clone();
    let blob = tokio::task::spawn_blocking(move || {
        db.require_vault_access(&vault_bytes, &account_id)?;
        db.get_onboarding_blob(&vault_bytes, &device_bytes)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    blob.map_or_else(
        || Err(Error::NotFound),
        |data| Ok(Json(serde_json::json!({ "blob": base64_encode(&data) }))),
    )
}

// ── Vault header bootstrap (issue #129) ──────────────────────────────

/// Request body for storing a vault header.
#[derive(Deserialize)]
pub struct PutVaultHeaderRequest {
    /// Base64-encoded non-secret vault header (KDF salts/params + wrapped VK).
    pub header: String,
}

/// `PUT /v1/vaults/:vault_id/header`
///
/// Upload the non-secret vault header so a new device can recover the vault
/// after SRP login. Authenticated and bound to the caller's account; the
/// server stores the bytes opaquely (the Vault Key inside is wrapped under
/// MEK←URK, never plaintext).
pub async fn put_vault_header(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(vault_id): Path<String>,
    Json(body): Json<PutVaultHeaderRequest>,
) -> Result<impl IntoResponse, Error> {
    let vault_bytes = parse_hex_16(&vault_id)?;
    let header = base64_decode(&body.header)?;
    if header.is_empty() {
        return Err(Error::BadRequest("header is required".into()));
    }
    let auth = authorize_sync(&state, &headers).await?;
    let account_id = auth.account_id;

    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.ensure_vault(&vault_bytes)?;
        db.claim_vault_for_account(&vault_bytes, &account_id)?;
        db.put_vault_header(&vault_bytes, &header)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

/// `GET /v1/vaults/:vault_id/header`
///
/// Download the stored vault header during new-device login. Authenticated and
/// authorized against the owning account.
pub async fn get_vault_header(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(vault_id): Path<String>,
) -> Result<impl IntoResponse, Error> {
    let vault_bytes = parse_hex_16(&vault_id)?;
    let auth = authorize_sync(&state, &headers).await?;
    let account_id = auth.account_id;

    let db = state.db.clone();
    let header = tokio::task::spawn_blocking(move || {
        db.require_vault_access(&vault_bytes, &account_id)?;
        db.get_vault_header(&vault_bytes)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    header.map_or_else(
        || Err(Error::NotFound),
        |data| Ok(Json(serde_json::json!({ "header": base64_encode(&data) }))),
    )
}

/// `GET /v1/account/header` — fetch the calling account's vault header.
///
/// A fresh device that has only just completed SRP login knows its account but
/// not the vault id, so it cannot use the vault-scoped route. The server
/// resolves the vault from the session's account and returns the non-secret
/// header (issue #129).
pub async fn get_account_vault_header(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, Error> {
    let auth = authorize_sync(&state, &headers).await?;
    let account_id = auth.account_id;
    let db = state.db.clone();
    let header = tokio::task::spawn_blocking(move || db.get_vault_header_for_account(&account_id))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;
    header.map_or_else(
        || Err(Error::NotFound),
        |data| Ok(Json(serde_json::json!({ "header": base64_encode(&data) }))),
    )
}
