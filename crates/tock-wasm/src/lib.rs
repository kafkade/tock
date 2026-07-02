//! WASM account bindings for the tock web app.
//!
//! Thin wrappers over the zero-I/O `tock-account` orchestration. HTTP lives in
//! the JS layer (`fetch`); these produce the request bodies and parse the
//! responses. Wire bodies cross the boundary as JSON strings the web posts
//! verbatim to the same server the CLI/Apple speak to. Secrets (bearer token,
//! channel binding) are hex; the password is never stored or returned.

use serde::Serialize;
use tock_account::{LoginPending, LoginStart, RotatePasswordMaterial, SetupCode, SignupMaterial};
use wasm_bindgen::prelude::*;

/// Map an account error to a JS error.
fn js_err(e: impl core::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Signup artifacts surfaced to the user once.
#[derive(Serialize)]
struct SignupBundle {
    register_request_json: String,
    emergency_kit_text: String,
    setup_code: String,
    secret_key: String,
}

/// Generate a fresh account and its artifacts. Web posts `register_request_json`
/// to `/v1/accounts/register`, then shows the Emergency Kit + Setup Code once.
///
/// # Errors
/// Returns a JS error string if derivation or serialization fails.
#[wasm_bindgen]
pub fn signup_account(
    username: &str,
    password: &str,
    server_url: &str,
) -> Result<JsValue, JsValue> {
    let (m, secret_key) =
        SignupMaterial::new_account(username, password, server_url).map_err(js_err)?;
    let bundle = SignupBundle {
        register_request_json: serde_json::to_string(&m.register_request).map_err(js_err)?,
        emergency_kit_text: m.emergency_kit.render_text(),
        setup_code: m.setup_code.encode(),
        secret_key,
    };
    serde_wasm_bindgen::to_value(&bundle).map_err(js_err)
}

/// Decoded Setup Code for prefilling a sign-in form.
#[derive(Serialize)]
struct ParsedSetupCode {
    server_url: String,
    email: String,
    secret_key: String,
}

/// Parse a `TOCK1:` Setup Code.
///
/// # Errors
/// Returns a JS error string for a malformed code.
#[wasm_bindgen]
pub fn parse_setup_code(code: &str) -> Result<JsValue, JsValue> {
    let c = SetupCode::parse(code).map_err(js_err)?;
    serde_wasm_bindgen::to_value(&ParsedSetupCode {
        server_url: c.server_url,
        email: c.email,
        secret_key: c.secret_key,
    })
    .map_err(js_err)
}

/// Re-encode a `TOCK1:` Setup Code from its parts.
///
/// Used by the self-service console to re-display the add-device code (and its
/// QR) for a signed-in user without a server round-trip — the code is a pure
/// function of these fields.
#[must_use]
#[wasm_bindgen]
pub fn build_setup_code(server_url: &str, email: &str, secret_key: &str) -> String {
    SetupCode {
        server_url: server_url.to_string(),
        email: email.to_string(),
        secret_key: secret_key.to_string(),
    }
    .encode()
}

/// Result of a password rotation: the re-wrapped vault header (base64) plus the
/// server-side SRP verifier update (JSON to POST).
#[derive(Serialize)]
struct RotationResult {
    /// Base64 of the re-wrapped `VaultHeader::to_bytes`, to upload verbatim.
    new_header_b64: String,
    /// JSON body for the server's SRP verifier-update endpoint.
    verifier_update_json: String,
    /// Whether a wrapped Vault Key was actually re-wrapped (vs. verifier-only).
    rewrapped_vk: bool,
}

/// Rotate the account password: re-wrap the Vault Key under the new password and
/// mint a fresh SRP verifier. The Secret Key is unchanged, so the Emergency Kit
/// and Setup Code stay valid.
///
/// `header_b64` is the current header from `GET /v1/account/header`. All crypto
/// runs here in WASM; the browser only uploads the results.
///
/// # Errors
/// Returns a JS error string for a malformed Secret Key or header, a wrong old
/// password (when a Vault Key is wrapped), or a crypto failure.
#[wasm_bindgen]
pub fn rotate_password(
    old_password: &str,
    new_password: &str,
    secret_key: &str,
    header_b64: &str,
) -> Result<JsValue, JsValue> {
    let (_aid, sk) = tock_account_secret_key(secret_key)?;
    let (new_header_b64, update, rewrapped_vk) =
        RotatePasswordMaterial::derive_from_header_b64(old_password, new_password, &sk, header_b64)
            .map_err(js_err)?;
    let result = RotationResult {
        new_header_b64,
        verifier_update_json: serde_json::to_string(&update).map_err(js_err)?,
        rewrapped_vk,
    };
    serde_wasm_bindgen::to_value(&result).map_err(js_err)
}

/// SRP login handle bridged to JS. Three round-trips: `start_json` → POST
/// `srp/start`; `finish` → POST `srp/finish`; `verify` → session material.
#[wasm_bindgen]
pub struct LoginSession {
    started: Option<LoginStart>,
    pending: Option<LoginPending>,
    start_request_json: String,
}

#[derive(Serialize)]
struct Session {
    bearer_token: String,
    channel_binding: String,
    expires_at: i64,
}

#[wasm_bindgen]
impl LoginSession {
    /// Begin an SRP login for `username`.
    ///
    /// # Errors
    /// Returns a JS error if the RNG fails.
    #[wasm_bindgen(constructor)]
    pub fn new(username: &str) -> Result<Self, JsValue> {
        let (start, req) = LoginStart::new(username).map_err(js_err)?;
        Ok(Self {
            started: Some(start),
            pending: None,
            start_request_json: serde_json::to_string(&req).map_err(js_err)?,
        })
    }

    /// The `srp/start` request body.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn start_request_json(&self) -> String {
        self.start_request_json.clone()
    }

    /// Feed the `srp/start` response; returns the `srp/finish` request body.
    ///
    /// # Errors
    /// Returns a JS error for malformed input or used-up session.
    pub fn finish(
        &mut self,
        start_response_json: &str,
        password: &str,
        secret_key: &str,
    ) -> Result<String, JsValue> {
        let (_aid, sk) = tock_account_secret_key(secret_key)?;
        let resp = serde_json::from_str(start_response_json).map_err(js_err)?;
        let start = self
            .started
            .take()
            .ok_or_else(|| JsValue::from_str("login already finished"))?;
        let (pending, req) = start.finish(&resp, password, &sk).map_err(js_err)?;
        self.pending = Some(pending);
        serde_json::to_string(&req).map_err(js_err)
    }

    /// Verify the `srp/finish` response; returns bearer + channel binding.
    ///
    /// # Errors
    /// Returns a JS error if mutual auth fails.
    pub fn verify(&mut self, finish_response_json: &str) -> Result<JsValue, JsValue> {
        let resp = serde_json::from_str(finish_response_json).map_err(js_err)?;
        let pending = self
            .pending
            .take()
            .ok_or_else(|| JsValue::from_str("login not ready to verify"))?;
        let m = pending.verify(&resp).map_err(js_err)?;
        serde_wasm_bindgen::to_value(&Session {
            bearer_token: m.bearer_token,
            channel_binding: m.channel_binding,
            expires_at: m.expires_at,
        })
        .map_err(js_err)
    }
}

fn tock_account_secret_key(s: &str) -> Result<([u8; 16], tock_account::SecretKey), JsValue> {
    tock_account::SecretKey::parse(s).map_err(|_| JsValue::from_str("malformed secret key"))
}
