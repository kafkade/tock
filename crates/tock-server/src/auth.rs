//! SRP login + authenticated-sync session layer (issue #130).
//!
//! This module turns the anonymous self-hosted sync endpoints into an
//! account-scoped, authenticated surface:
//!
//! - [`srp_start`] / [`srp_finish`] run the SRP-6a mutual-auth handshake
//!   (ADR-010) against the verifier stored at registration (the #127
//!   `get_srp_credentials` seam). A successful `finish` mints a short-lived
//!   **bearer session token derived from the SRP session key `K`**
//!   ([`tock_crypto::srp::Session::derive_bearer_token`]).
//! - [`refresh`] slides a live session's expiry forward.
//! - [`authorize_sync`] is the guard the sync routes call to require a valid
//!   session (self-hosted) or hosted API token before touching a vault.
//!
//! ## Zero-knowledge & secret hygiene
//!
//! The server only ever sees SRP public values (`A`, `B`, `M1`, `M2`, salt,
//! verifier) and ciphertext. The bearer token is **never** returned in a
//! response body — both sides derive it independently from `K` — and only its
//! SHA-256 hash is persisted, so a database leak yields nothing usable. The
//! server-secret ephemeral `b` lives in memory only, between `start` and
//! `finish`, and is dropped (zeroized) immediately after.

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tock_crypto::srp::ServerHandshake;

use crate::accounts::{PeerAddr, client_ip};
use crate::billing::{ServerMode, Tier};
use crate::codec::{base64_decode, base64_encode, hex_decode, hex_encode};
use crate::error::Error;
use crate::state::AppState;

/// Lifetime of a sync bearer session, in seconds (re-login or [`refresh`]
/// extends access beyond this).
const SESSION_TTL_SECS: i64 = 3600;

/// Lifetime of a pending handshake awaiting its `finish`, in seconds. Short by
/// design: the server-secret ephemeral `b` should not outlive the round-trip.
const PENDING_TTL_SECS: i64 = 120;

/// SRP group/hash identifier this server implements (the single group provided
/// by `tock_crypto::srp`). Accounts are registered against it.
const SUPPORTED_SRP_GROUP: &str = "RFC5054-4096-SHA256";

/// Header carrying the SRP channel-binding tag on event routes (ADR-010
/// defense-in-depth against TLS-stripping).
const CHANNEL_BINDING_HEADER: &str = "x-tock-channel-binding";

/// Server-side state for a login between `start` and `finish`. Held in memory
/// only; never persisted.
pub struct PendingHandshake {
    /// The crypto handshake holding the secret ephemeral `b`.
    handshake: ServerHandshake,
    /// Account the login resolves to.
    account_id: String,
    /// SRP identity `I` (the username bytes) bound into `M1`/`M2`.
    identity: Vec<u8>,
    /// SRP salt for this account.
    salt: Vec<u8>,
    /// SRP verifier for this account.
    verifier: Vec<u8>,
    /// Client public ephemeral `A` from `start`.
    a_pub: Vec<u8>,
    /// Absolute expiry (Unix seconds).
    expires_at: i64,
}

/// Current Unix time in whole seconds.
fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// Hex SHA-256 of a byte string (used to index sessions by token hash).
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

/// Constant-time byte-slice equality (length-independent on the secret).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

// ── SRP login: start ─────────────────────────────────────────────────

/// Request body for `POST /v1/auth/srp/start`.
#[derive(Deserialize)]
pub struct StartRequest {
    /// Login identifier (the SRP identity `I`).
    pub username: String,
    /// Base64-encoded client public ephemeral `A`.
    pub a_pub: String,
}

/// Response body for `POST /v1/auth/srp/start`.
#[derive(Serialize)]
pub struct StartResponse {
    /// Opaque id correlating this `start` with the matching `finish`.
    pub handshake_id: String,
    /// Base64-encoded SRP salt for the account.
    pub salt: String,
    /// Base64-encoded server public ephemeral `B`.
    pub b_pub: String,
    /// Opaque KDF parameters the account stored at registration, so a fresh
    /// device can re-derive the Unlock Root Key before completing login
    /// (issue #129). `null` for accounts registered without them.
    pub kdf_params: serde_json::Value,
}

/// `POST /v1/auth/srp/start` — begin an SRP login.
///
/// Looks up the account's stored verifier, computes `B = k*v + g^b`, and
/// returns it with `salt`. Unknown users and accounts without SRP credentials
/// are rejected uniformly as `401` (see the username-enumeration note in the
/// issue follow-ups). Rate-limited per client IP.
pub async fn srp_start(
    State(state): State<AppState>,
    PeerAddr(peer): PeerAddr,
    headers: HeaderMap,
    Json(body): Json<StartRequest>,
) -> Result<Json<StartResponse>, Error> {
    let username = body.username.trim().to_string();
    if username.is_empty() {
        return Err(Error::BadRequest("username is required".into()));
    }
    let a_pub = base64_decode(&body.a_pub)?;
    if a_pub.is_empty() {
        return Err(Error::BadRequest("a_pub is required".into()));
    }

    let ip = client_ip(&headers, peer);
    if !state
        .rate_limiter
        .check(&format!("srp-start:{ip}"), Tier::Free)
    {
        return Err(Error::RateLimited);
    }

    let db = state.db.clone();
    let lookup_username = username.clone();
    let prepared = tokio::task::spawn_blocking(move || {
        let creds = db
            .get_srp_credentials(&lookup_username)?
            .ok_or(Error::Unauthorized("invalid credentials"))?;
        // We implement a single SRP group; reject accounts registered against a
        // different one rather than silently producing an unusable handshake.
        if !creds.srp_group.is_empty() && creds.srp_group != SUPPORTED_SRP_GROUP {
            return Err(Error::Internal(format!(
                "unsupported SRP group: {}",
                creds.srp_group
            )));
        }
        // Modular exponentiation (~50ms) runs here, off the async runtime.
        let handshake = ServerHandshake::new(&creds.srp_verifier)
            .map_err(|e| Error::Internal(e.to_string()))?;
        let b_pub = handshake.public().to_vec();
        Ok::<_, Error>((creds, handshake, b_pub))
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    let (creds, handshake, b_pub) = prepared;
    let kdf_params: serde_json::Value =
        serde_json::from_str(&creds.kdf_params).unwrap_or(serde_json::Value::Null);
    let handshake_id = uuid::Uuid::new_v4().to_string();
    let now = now_unix();
    let pending = PendingHandshake {
        handshake,
        account_id: creds.account_id,
        identity: username.into_bytes(),
        salt: creds.srp_salt.clone(),
        verifier: creds.srp_verifier,
        a_pub,
        expires_at: now + PENDING_TTL_SECS,
    };

    {
        let mut map = state
            .pending
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        // Opportunistically drop stale handshakes so the map can't grow without
        // bound from abandoned logins.
        map.retain(|_, h| h.expires_at > now);
        map.insert(handshake_id.clone(), pending);
    }

    Ok(Json(StartResponse {
        handshake_id,
        salt: base64_encode(&creds.srp_salt),
        b_pub: base64_encode(&b_pub),
        kdf_params,
    }))
}

// ── SRP login: finish ────────────────────────────────────────────────

/// Request body for `POST /v1/auth/srp/finish`.
#[derive(Deserialize)]
pub struct FinishRequest {
    /// The `handshake_id` returned by `start`.
    pub handshake_id: String,
    /// Base64-encoded client proof `M1`.
    pub m1: String,
}

/// Response body for `POST /v1/auth/srp/finish`.
#[derive(Serialize)]
pub struct FinishResponse {
    /// Base64-encoded server proof `M2` (mutual authentication).
    pub m2: String,
    /// Absolute session expiry (Unix seconds). The client derives the bearer
    /// token itself from the shared key `K`; it is never sent here.
    pub expires_at: i64,
}

/// `POST /v1/auth/srp/finish` — complete an SRP login.
///
/// Verifies `M1`, returns `M2`, and mints a session whose bearer token is
/// `derive_bearer_token(K)` — persisted only as a hash alongside the
/// channel-binding tag and expiry.
pub async fn srp_finish(
    State(state): State<AppState>,
    Json(body): Json<FinishRequest>,
) -> Result<Json<FinishResponse>, Error> {
    let m1 = base64_decode(&body.m1)?;
    let now = now_unix();

    // Consume the pending handshake up front: a handshake id is single-use, so
    // a replayed or concurrent `finish` finds nothing.
    let pending = {
        let mut map = state
            .pending
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        map.remove(&body.handshake_id)
    };
    let pending = pending.ok_or(Error::Unauthorized("unknown or expired handshake"))?;
    if pending.expires_at <= now {
        return Err(Error::Unauthorized("unknown or expired handshake"));
    }

    let db = state.db.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let (session, m2) = pending
            .handshake
            .verify(
                &pending.identity,
                &pending.salt,
                &pending.a_pub,
                &pending.verifier,
                &m1,
            )
            .map_err(|_| Error::Unauthorized("invalid credentials"))?;

        let bearer = session
            .derive_bearer_token()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let channel_binding = session
            .derive_channel_binding()
            .map_err(|e| Error::Internal(e.to_string()))?;
        let token_hash = sha256_hex(bearer.expose_secret());
        let expires_at = now + SESSION_TTL_SECS;

        db.create_session(
            &token_hash,
            &pending.account_id,
            &channel_binding,
            expires_at,
        )?;
        // Opportunistic housekeeping so expired rows don't accumulate.
        let _ = db.delete_expired_sessions(now);
        Ok::<_, Error>((m2, expires_at))
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    let (m2, expires_at) = outcome;
    Ok(Json(FinishResponse {
        m2: base64_encode(&m2),
        expires_at,
    }))
}

// ── Session refresh ──────────────────────────────────────────────────

/// Response body for `POST /v1/auth/refresh`.
#[derive(Serialize)]
pub struct RefreshResponse {
    /// New absolute session expiry (Unix seconds).
    pub expires_at: i64,
}

/// `POST /v1/auth/refresh` — slide the presented session's expiry forward.
///
/// Requires a still-valid bearer token; an expired session must re-login (the
/// bearer token rotates only when `K` does).
pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RefreshResponse>, Error> {
    let token_hash = bearer_token_hash(&headers)?;
    let now = now_unix();
    let new_expires_at = now + SESSION_TTL_SECS;

    let db = state.db.clone();
    let refreshed =
        tokio::task::spawn_blocking(move || db.refresh_session(&token_hash, now, new_expires_at))
            .await
            .map_err(|e| Error::Internal(e.to_string()))??;

    if refreshed {
        Ok(Json(RefreshResponse {
            expires_at: new_expires_at,
        }))
    } else {
        Err(Error::Unauthorized("invalid or expired session"))
    }
}

// ── Authorization guard for sync routes ──────────────────────────────

/// The authenticated principal behind a sync request.
pub struct SyncAuth {
    /// Account that owns the request.
    pub account_id: String,
    /// Channel-binding tag for SRP sessions (`None` for hosted API-token
    /// callers, which predate the SRP layer).
    pub channel_binding: Option<Vec<u8>>,
}

/// Extract and validate the bearer token, returning its SHA-256 hash for a
/// session lookup. Shared by [`refresh`] and [`authorize_sync`].
fn bearer_token_hash(headers: &HeaderMap) -> Result<String, Error> {
    let token = bearer_token(headers)?;
    // The presented token is a hex-encoded 32-byte value derived from `K`; we
    // index sessions by its hash, so the raw secret never lands in the DB.
    let bytes = hex_decode(&token)
        .map_err(|_| Error::Unauthorized("malformed bearer token"))
        .and_then(|b| {
            if b.is_empty() {
                Err(Error::Unauthorized("malformed bearer token"))
            } else {
                Ok(b)
            }
        })?;
    Ok(sha256_hex(&bytes))
}

/// Pull the raw `Authorization: Bearer <token>` value out of the headers.
fn bearer_token(headers: &HeaderMap) -> Result<String, Error> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)
        .ok_or(Error::Unauthorized("missing bearer token"))?;
    let value = raw
        .to_str()
        .map_err(|_| Error::Unauthorized("invalid authorization header"))?;
    value
        .strip_prefix("Bearer ")
        .filter(|t| !t.is_empty())
        .map(ToString::to_string)
        .ok_or(Error::Unauthorized("expected bearer token"))
}

/// Authorize a sync request, resolving the owning account.
///
/// - **Hosted mode** keeps the API-token path (issued by the billing flow).
/// - **Self-hosted mode** requires a valid SRP session bearer token.
///
/// On success the caller still enforces vault ownership via
/// [`crate::db::ServerDb::require_vault_access`] /
/// [`crate::db::ServerDb::claim_vault_for_account`].
pub async fn authorize_sync(state: &AppState, headers: &HeaderMap) -> Result<SyncAuth, Error> {
    if state.mode == ServerMode::Hosted {
        let token = bearer_token(headers)?;
        let db = state.db.clone();
        let account_id = tokio::task::spawn_blocking(move || db.account_id_by_api_token(&token))
            .await
            .map_err(|e| Error::Internal(e.to_string()))??
            .ok_or(Error::Unauthorized("invalid bearer token"))?;
        return Ok(SyncAuth {
            account_id,
            channel_binding: None,
        });
    }

    let token_hash = bearer_token_hash(headers)?;
    let now = now_unix();
    let db = state.db.clone();
    let record = tokio::task::spawn_blocking(move || db.lookup_session(&token_hash, now))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??
        .ok_or(Error::Unauthorized("invalid or expired session"))?;
    Ok(SyncAuth {
        account_id: record.account_id,
        channel_binding: Some(record.channel_binding),
    })
}

/// Enforce the SRP channel-binding tag on event routes (ADR-010
/// defense-in-depth). For hosted API-token callers (`channel_binding == None`)
/// this is a no-op. For SRP sessions the request MUST carry
/// `X-Tock-Channel-Binding: <hex tag>` matching the session's tag.
pub fn verify_channel_binding(auth: &SyncAuth, headers: &HeaderMap) -> Result<(), Error> {
    let Some(expected) = auth.channel_binding.as_deref() else {
        return Ok(());
    };
    let raw = headers
        .get(CHANNEL_BINDING_HEADER)
        .ok_or(Error::Unauthorized("missing channel-binding tag"))?;
    let presented = raw
        .to_str()
        .ok()
        .and_then(|s| hex_decode(s).ok())
        .ok_or(Error::Unauthorized("malformed channel-binding tag"))?;
    if ct_eq(&presented, expected) {
        Ok(())
    } else {
        Err(Error::Unauthorized("channel-binding mismatch"))
    }
}

/// Authorize an admin request via an SRP session token, returning the admin
/// account id when the session's account is an active admin. Used by the admin
/// guard alongside the interim admin API token.
pub async fn admin_via_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<String>, Error> {
    let Ok(token_hash) = bearer_token_hash(headers) else {
        return Ok(None);
    };
    let now = now_unix();
    let db = state.db.clone();
    let record = tokio::task::spawn_blocking(move || db.lookup_session(&token_hash, now))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;
    Ok(record.filter(|r| r.role == "admin").map(|r| r.account_id))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{ct_eq, sha256_hex};

    #[test]
    fn ct_eq_matches_only_identical_slices() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }

    #[test]
    fn sha256_hex_is_stable_and_64_chars() {
        let h = sha256_hex(b"token-bytes");
        assert_eq!(h.len(), 64);
        assert_eq!(h, sha256_hex(b"token-bytes"));
        assert_ne!(h, sha256_hex(b"other"));
    }
}
