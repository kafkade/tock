//! Route handlers for the sync API.
//!
//! All data the server handles is opaque — encrypted blobs that it
//! stores and serves without ever decrypting.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

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
    Path(vault_id): Path<String>,
    Json(body): Json<RegisterDeviceRequest>,
) -> Result<impl IntoResponse, Error> {
    let vault_bytes = parse_hex_16(&vault_id)?;
    let device_bytes = parse_hex_16(&body.device_id)?;
    let vk_bytes = parse_hex_32(&body.verifying_key)?;

    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.ensure_vault(&vault_bytes)?;
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
    Path(vault_id): Path<String>,
    Json(body): Json<PushEventsRequest>,
) -> Result<Json<PushResponse>, Error> {
    let vault_bytes = parse_hex_16(&vault_id)?;

    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || {
        db.ensure_vault(&vault_bytes)?;
        let mut accepted = 0_usize;
        let mut duplicates = 0_usize;
        for item in &body.events {
            let eid = parse_hex_16(&item.event_id)?;
            let did = parse_hex_16(&item.device_id)?;
            let payload = base64_decode(&item.payload)?;
            if db.push_event(&eid, &vault_bytes, &did, item.lamport, &payload)? {
                accepted += 1;
            } else {
                duplicates += 1;
            }
        }
        let server_lamport = db.max_lamport(&vault_bytes)?;
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
    /// Return events with lamport > this value. Defaults to 0.
    #[serde(default)]
    pub after_lamport: i64,
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
    /// Whether there are more events.
    pub more: bool,
}

/// `GET /v1/vaults/:vault_id/events/pull`
pub async fn pull_events(
    State(state): State<AppState>,
    Path(vault_id): Path<String>,
    Query(query): Query<PullQuery>,
) -> Result<Json<PullResponse>, Error> {
    let vault_bytes = parse_hex_16(&vault_id)?;
    let limit = query.limit.min(256);

    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || {
        // Request limit+1 to detect "more".
        let events = db.pull_events(&vault_bytes, query.after_lamport, limit + 1)?;
        let more = events.len() > limit;
        let items: Vec<PullEventItem> = events
            .into_iter()
            .take(limit)
            .map(|e| PullEventItem {
                event_id: hex_encode(&e.id),
                device_id: hex_encode(&e.device_id),
                lamport: e.lamport,
                payload: base64_encode(&e.payload),
                created_at: e.created_at,
            })
            .collect();
        Ok::<_, Error>(PullResponse {
            events: items,
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
    Path((vault_id, device_id)): Path<(String, String)>,
    Json(body): Json<PutOnboardingBlobRequest>,
) -> Result<impl IntoResponse, Error> {
    let vault_bytes = parse_hex_16(&vault_id)?;
    let device_bytes = parse_hex_16(&device_id)?;
    let blob = base64_decode(&body.blob)?;

    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.ensure_vault(&vault_bytes)?;
        db.put_onboarding_blob(&vault_bytes, &device_bytes, &blob)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

/// `GET /v1/vaults/:vault_id/onboarding/:device_id`
pub async fn get_onboarding_blob(
    State(state): State<AppState>,
    Path((vault_id, device_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, Error> {
    let vault_bytes = parse_hex_16(&vault_id)?;
    let device_bytes = parse_hex_16(&device_id)?;

    let db = state.db.clone();
    let blob =
        tokio::task::spawn_blocking(move || db.get_onboarding_blob(&vault_bytes, &device_bytes))
            .await
            .map_err(|e| Error::Internal(e.to_string()))??;

    blob.map_or_else(
        || Err(Error::NotFound),
        |data| Ok(Json(serde_json::json!({ "blob": base64_encode(&data) }))),
    )
}

// ── Helpers ──────────────────────────────────────────────────────────

fn parse_hex_16(s: &str) -> Result<[u8; 16], Error> {
    let bytes = hex_decode(s)?;
    bytes
        .try_into()
        .map_err(|_| Error::BadRequest("expected 16-byte hex value".into()))
}

fn parse_hex_32(s: &str) -> Result<[u8; 32], Error> {
    let bytes = hex_decode(s)?;
    bytes
        .try_into()
        .map_err(|_| Error::BadRequest("expected 32-byte hex value".into()))
}

fn hex_decode(s: &str) -> Result<Vec<u8>, Error> {
    if !s.len().is_multiple_of(2) {
        return Err(Error::BadRequest("odd-length hex string".into()));
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().chunks(2) {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        bytes.push((hi << 4) | lo);
    }
    Ok(bytes)
}

fn hex_nibble(b: u8) -> Result<u8, Error> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(Error::BadRequest("invalid hex character".into())),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

fn base64_decode(s: &str) -> Result<Vec<u8>, Error> {
    // Simple base64 decode (standard alphabet + padding).
    base64_decode_impl(s).map_err(|()| Error::BadRequest("invalid base64".into()))
}

fn base64_decode_impl(s: &str) -> Result<Vec<u8>, ()> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for ch in s.bytes() {
        let val = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'\n' | b'\r' | b' ' => continue,
            _ => return Err(()),
        };
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    Ok(out)
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = if chunk.len() > 1 {
            u32::from(chunk[1])
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            u32::from(chunk[2])
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((triple >> 18) & 0x3F) as usize]);
        out.push(ALPHABET[((triple >> 12) & 0x3F) as usize]);
        if chunk.len() > 1 {
            out.push(ALPHABET[((triple >> 6) & 0x3F) as usize]);
        } else {
            out.push(b'=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(triple & 0x3F) as usize]);
        } else {
            out.push(b'=');
        }
    }
    String::from_utf8(out).unwrap_or_default()
}
