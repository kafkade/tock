//! Login / add-device orchestration: an SRP-6a client state machine plus the
//! request/response DTOs the server already speaks.
//!
//! Flow: [`LoginStart::new`] → send [`StartRequest`] → feed [`StartResponse`]
//! into [`LoginStart::finish`] → send [`FinishRequest`] → feed
//! [`FinishResponse`] into [`LoginPending::verify`] → [`SessionMaterial`].
//! The chicken-and-egg of new-device login (need URK to derive the SRP input,
//! but the URK needs the KDF salt) is resolved by the `kdf_params` the server
//! returns in [`StartResponse`].

use serde::{Deserialize, Serialize};
use tock_crypto::SecretKey;
use tock_crypto::kdf::derive_srp_input;
use tock_crypto::srp::{ClientHandshake, ClientLogin};

use crate::codec::{base64_decode, base64_encode, hex_encode};
use crate::error::AccountError;
use crate::kdf_params::KdfParams;

/// Request body for `POST /v1/auth/srp/start`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StartRequest {
    /// Login identifier.
    pub username: String,
    /// Base64 client public ephemeral `A`.
    pub a_pub: String,
}

/// Response body for `POST /v1/auth/srp/start`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StartResponse {
    /// Opaque handshake id.
    pub handshake_id: String,
    /// Base64 SRP salt.
    pub salt: String,
    /// Base64 server public ephemeral `B`.
    pub b_pub: String,
    /// Opaque KDF params for re-deriving the URK on a fresh device.
    pub kdf_params: serde_json::Value,
}

/// Request body for `POST /v1/auth/srp/finish`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FinishRequest {
    /// Handshake id from start.
    pub handshake_id: String,
    /// Base64 client proof `M1`.
    pub m1: String,
}

/// Response body for `POST /v1/auth/srp/finish`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FinishResponse {
    /// Base64 server proof `M2`.
    pub m2: String,
    /// Absolute session expiry (Unix seconds).
    pub expires_at: i64,
}

/// Authenticated material for a logged-in device. Secrets are hex-encoded for
/// HTTP headers; the password is never present.
pub struct SessionMaterial {
    /// Bearer token (hex) → `Authorization: Bearer <…>`.
    pub bearer_token: String,
    /// Channel-binding tag (hex) → `X-Tock-Channel-Binding: <…>`.
    pub channel_binding: String,
    /// Absolute session expiry (Unix seconds).
    pub expires_at: i64,
}

/// Begun login: holds the SRP client ephemeral until the server replies.
pub struct LoginStart {
    handshake: ClientHandshake,
    username: String,
}

impl LoginStart {
    /// Begin a login and produce the `srp/start` request.
    ///
    /// # Errors
    /// Returns [`AccountError::Crypto`] if the RNG fails.
    pub fn new(username: &str) -> Result<(Self, StartRequest), AccountError> {
        let handshake = ClientHandshake::new()?;
        let a_pub = base64_encode(handshake.public());
        Ok((
            Self {
                handshake,
                username: username.to_string(),
            },
            StartRequest {
                username: username.to_string(),
                a_pub,
            },
        ))
    }

    /// Process the server's start response: re-derive the URK from the
    /// returned KDF params, compute the SRP input, and produce `M1`.
    ///
    /// # Errors
    /// Returns [`AccountError::KdfParams`] if the server returned none,
    /// [`AccountError::Encoding`] for bad base64, or [`AccountError::Crypto`].
    pub fn finish(
        self,
        resp: &StartResponse,
        password: &str,
        secret_key: &SecretKey,
    ) -> Result<(LoginPending, FinishRequest), AccountError> {
        let kdf = KdfParams::from_json(&resp.kdf_params)?;
        let salt = base64_decode(&resp.salt)?;
        let b_pub = base64_decode(&resp.b_pub)?;
        let urk = kdf.derive_urk(password.as_bytes(), secret_key)?;
        let srp_x = derive_srp_input(&urk, &salt)?;
        let login = self
            .handshake
            .finish(self.username.as_bytes(), &salt, &b_pub, &srp_x)?;
        let m1 = base64_encode(login.proof());
        Ok((
            LoginPending { login },
            FinishRequest {
                handshake_id: resp.handshake_id.clone(),
                m1,
            },
        ))
    }
}

/// Login awaiting the server proof `M2`.
pub struct LoginPending {
    login: ClientLogin,
}

impl LoginPending {
    /// Verify `M2` (mutual auth) and derive the session material.
    ///
    /// # Errors
    /// Returns [`AccountError::Auth`] if `M2` does not match, or
    /// [`AccountError::Crypto`] if bearer/channel-binding derivation fails.
    pub fn verify(self, resp: &FinishResponse) -> Result<SessionMaterial, AccountError> {
        let m2 = base64_decode(&resp.m2)?;
        let session = self
            .login
            .verify_server(&m2)
            .map_err(|_| AccountError::Auth)?;
        let bearer = session.derive_bearer_token()?;
        let channel = session.derive_channel_binding()?;
        Ok(SessionMaterial {
            bearer_token: hex_encode(bearer.expose_secret()),
            channel_binding: hex_encode(&channel),
            expires_at: resp.expires_at,
        })
    }
}
