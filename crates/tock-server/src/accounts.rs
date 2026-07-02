//! Account endpoints.
//!
//! Two surfaces share this module:
//!
//! - **Self-hosted account registration** (`POST /v1/accounts/register`),
//!   available in every mode. It stores the client-computed SRP verifier
//!   (ADR-011) — never a password, Secret Key, or 2SKD root — and applies the
//!   instance registration policy with first-run admin bootstrap.
//! - **Hosted billing accounts** (`POST /v1/accounts`, `GET /v1/accounts/:id`,
//!   `/usage`), active only under `--mode hosted`, retaining the API-token +
//!   subscription-tier model.

use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::billing::{ServerMode, Tier, UsageSnapshot};
use crate::codec::base64_decode;
use crate::db::NewAccount;
use crate::error::Error;
use crate::state::AppState;

/// Infallible extractor for the peer socket address, when the service was built
/// with connect info (it always is via [`crate::serve`]). Returns `None` for
/// router-only callers (e.g. tower `oneshot` tests) so handlers never fail.
#[allow(clippy::redundant_pub_crate)]
pub(crate) struct PeerAddr(pub(crate) Option<SocketAddr>);

impl<S: Send + Sync> FromRequestParts<S> for PeerAddr {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|c| c.0),
        ))
    }
}

/// Who may create an account on a self-hosted instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationPolicy {
    /// Anyone may self-register (an invite, if supplied, still applies).
    Open,
    /// Registration requires a valid admin-issued invite token.
    InviteOnly,
    /// Self-registration is off; only admin-issued invites can register a user.
    Disabled,
}

impl RegistrationPolicy {
    /// Canonical name for storage / display.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InviteOnly => "invite-only",
            Self::Disabled => "disabled",
        }
    }

    /// Parse from a string; `None` for unrecognized values.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "invite-only" | "inviteonly" => Some(Self::InviteOnly),
            "disabled" | "closed" => Some(Self::Disabled),
            _ => None,
        }
    }
}

impl std::fmt::Display for RegistrationPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Default SRP group identifier when a client omits one. Matches the single
/// group implemented by `tock_crypto::srp` (RFC 5054 4096-bit, SHA-256).
fn default_srp_group() -> String {
    "RFC5054-4096-SHA256".to_string()
}

/// Request body for self-hosted account registration.
#[derive(Deserialize)]
pub struct RegisterRequest {
    /// Login identifier (email or bare username).
    pub username: String,
    /// Base64-encoded SRP salt (`salt_srp`).
    pub srp_salt: String,
    /// Base64-encoded SRP verifier `v = g^x mod N`.
    pub srp_verifier: String,
    /// SRP group/hash identifier; defaults to `RFC5054-4096-SHA256`.
    #[serde(default = "default_srp_group")]
    pub srp_group: String,
    /// Opaque KDF parameters the client needs to re-derive the URK (stored
    /// verbatim; never interpreted by the server).
    #[serde(default)]
    pub kdf_params: serde_json::Value,
    /// Invite token (required under invite-only / disabled policies).
    #[serde(default)]
    pub invite_token: Option<String>,
    /// Hex vault id the optional `header` is stored under (issue #129/#131).
    #[serde(default)]
    pub vault_id: Option<String>,
    /// Base64 non-secret vault header to store at registration so a new device
    /// can log in (issue #129) and the password can later be rotated (#131).
    /// Ignored unless `vault_id` is also present.
    #[serde(default)]
    pub header: Option<String>,
}

/// Response for a successful registration.
#[derive(Serialize)]
pub struct RegisterResponse {
    /// Server-assigned account id.
    pub account_id: String,
    /// Granted role (`admin` or `user`).
    pub role: String,
    /// Account status (`active`).
    pub status: String,
    /// Interim admin bearer token, present only for the bootstrapped admin
    /// (issue #130 replaces this with SRP session tokens).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_token: Option<String>,
}

/// Best-effort client IP for rate limiting: trust `X-Forwarded-For` (typical
/// self-host reverse-proxy deployment) and fall back to the socket peer.
#[allow(clippy::redundant_pub_crate)]
pub(crate) fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
    if let Some(first) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|xff| xff.split(',').next())
    {
        let ip = first.trim();
        if !ip.is_empty() {
            return ip.to_string();
        }
    }
    peer.map_or_else(|| "unknown".to_string(), |addr| addr.ip().to_string())
}

/// `POST /v1/accounts/register` — register a self-hosted account.
///
/// Rate-limited per client IP via the shared limiter. The first account on a
/// fresh instance is bootstrapped as an admin (Immich pattern); thereafter the
/// configured [`RegistrationPolicy`] governs who may register.
pub async fn register(
    State(state): State<AppState>,
    PeerAddr(peer): PeerAddr,
    headers: HeaderMap,
    Json(body): Json<RegisterRequest>,
) -> Result<impl IntoResponse, Error> {
    let username = body.username.trim().to_string();
    if username.is_empty() {
        return Err(Error::BadRequest("username is required".into()));
    }
    let salt = base64_decode(&body.srp_salt)?;
    let verifier = base64_decode(&body.srp_verifier)?;
    if salt.is_empty() || verifier.is_empty() {
        return Err(Error::BadRequest(
            "srp_salt and srp_verifier are required".into(),
        ));
    }

    let client = client_ip(&headers, peer);
    if !state
        .rate_limiter
        .check(&format!("register:{client}"), Tier::Free)
    {
        return Err(Error::RateLimited);
    }

    let kdf_params = body.kdf_params.to_string();
    let srp_group = body.srp_group;
    let invite = body.invite_token;

    // Optional vault header uploaded at registration (issue #129/#131). Decoded
    // up front so a malformed value is a clean 400 before we touch the account.
    let header_upload = match (body.vault_id.as_deref(), body.header.as_deref()) {
        (Some(vault_id), Some(header_b64)) if !header_b64.is_empty() => {
            let vault_bytes = crate::codec::parse_hex_16(vault_id)?;
            let header = base64_decode(header_b64)?;
            Some((vault_bytes, header))
        }
        _ => None,
    };

    let db = state.db.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let policy = db.registration_policy()?;
        let outcome = db.register_account(
            &NewAccount {
                username: &username,
                srp_salt: &salt,
                srp_verifier: &verifier,
                srp_group: &srp_group,
                kdf_params: &kdf_params,
                invite_token: invite.as_deref(),
            },
            policy,
        )?;
        if let Some((vault_bytes, header)) = header_upload {
            db.ensure_vault(&vault_bytes)?;
            db.claim_vault_for_account(&vault_bytes, &outcome.account_id)?;
            db.put_vault_header(&vault_bytes, &header)?;
        }
        Ok::<_, Error>(outcome)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    Ok((
        StatusCode::CREATED,
        Json(RegisterResponse {
            account_id: outcome.account_id,
            role: outcome.role,
            status: outcome.status,
            admin_token: outcome.admin_token,
        }),
    ))
}

// ── Hosted billing accounts (--mode hosted only) ─────────────────────

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
