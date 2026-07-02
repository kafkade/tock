//! Signup orchestration: registration material, Emergency Kit, Setup Code.
//!
//! All pure: this module turns `(username, password, SecretKey, VaultHeader)`
//! into the request body the server's `POST /v1/accounts/register` already
//! accepts, plus the two user-facing artifacts (Emergency Kit + Setup Code).
//! Generating/printing the PDF and rendering the QR happen at the I/O edge.

use serde::{Deserialize, Serialize};
use tock_core::vault::{KeyHierarchy, VaultHeader};
use tock_crypto::SecretKey;
use tock_crypto::kdf::derive_srp_input;
use tock_crypto::random::fill_random;
use tock_crypto::srp::compute_verifier;

use crate::codec::base64_encode;
use crate::error::AccountError;
use crate::kdf_params::KdfParams;

/// SRP group/hash identifier supported by the server.
pub const SRP_GROUP: &str = "RFC5054-4096-SHA256";

/// Build an in-memory v2 vault header for a brand-new account (no I/O). Mirrors
/// the parameters `tock-storage::init` stamps so server-side accounts created
/// from the web match native ones.
fn fresh_header() -> Result<VaultHeader, AccountError> {
    use tock_core::vault::Argon2HeaderParams;
    use tock_core::vault::header::{FORMAT_VERSION, MAGIC, MIN_COMPAT_VERSION, STORAGE_LAYOUT_V0};
    let mut kdf_salt = [0u8; 16];
    let mut hkdf_salt = [0u8; 32];
    fill_random(&mut kdf_salt)?;
    fill_random(&mut hkdf_salt)?;
    let mut id_bytes = [0u8; 16];
    fill_random(&mut id_bytes)?;
    let id = uuid::Uuid::from_bytes(id_bytes);
    Ok(VaultHeader {
        magic: MAGIC,
        format_version: FORMAT_VERSION,
        min_compatible_version: MIN_COMPAT_VERSION,
        vault_id: id,
        account_id: id,
        kdf_version: 1,
        kdf_salt,
        hkdf_salt,
        argon2: Argon2HeaderParams {
            t: 3,
            m_kib: 65_536,
            p: 1,
        },
        vk_wrap_nonce: [0; 12],
        vk_wrap_ct: Vec::new(),
        created_at: time::OffsetDateTime::UNIX_EPOCH,
        storage_layout: STORAGE_LAYOUT_V0.to_string(),
    })
}

/// Everything signup produces: the wire request plus user artifacts.
pub struct SignupMaterial {
    /// Request body for `POST /v1/accounts/register`.
    pub register_request: RegisterRequest,
    /// Printable + textual Emergency Kit.
    pub emergency_kit: EmergencyKit,
    /// Setup Code for fast add-device.
    pub setup_code: SetupCode,
    /// The freshly minted vault header (non-secret: KDF salts/params + an empty
    /// VK wrap for a brand-new browser account). Uploaded at registration so a
    /// new device can log in (issue #129) and the password can later be rotated
    /// (issue #131).
    pub header: VaultHeader,
}

impl SignupMaterial {
    /// Generate a brand-new account from scratch: sample a Secret Key and a
    /// fresh vault header in memory, then derive signup material. Used by edges
    /// without local `SQLite` storage (web/`WASM`); native CLI/Apple init a real
    /// vault first and call [`SignupMaterial::derive`].
    ///
    /// Returns the material plus the `A4-…` Secret-Key string to surface once.
    ///
    /// # Errors
    /// Returns [`AccountError::Crypto`] if RNG or 2SKD derivation fails.
    pub fn new_account(
        username: &str,
        password: &str,
        server_url: &str,
    ) -> Result<(Self, String), AccountError> {
        let secret_key = SecretKey::generate()?;
        let header = fresh_header()?;
        let material = Self::derive(username, password, &secret_key, &header, server_url)?;
        let kit = secret_key.to_emergency_kit(header.account_id.as_bytes());
        Ok((material, kit))
    }

    /// Derive all signup material from a freshly created local vault.
    ///
    /// `header` is the just-initialised vault header; `secret_key` is the one
    /// returned by `tock_storage::init`. A random `salt_srp` is sampled here.
    ///
    /// # Errors
    /// Returns [`AccountError::Crypto`] if RNG or 2SKD derivation fails.
    pub fn derive(
        username: &str,
        password: &str,
        secret_key: &SecretKey,
        header: &VaultHeader,
        server_url: &str,
    ) -> Result<Self, AccountError> {
        let mut salt_srp = [0u8; 16];
        fill_random(&mut salt_srp)?;
        let urk = KeyHierarchy::derive_unlock_root_key(password.as_bytes(), secret_key, header)?;
        let srp_x = derive_srp_input(&urk, &salt_srp)?;
        let verifier = compute_verifier(&srp_x);
        let kdf_params = KdfParams::from_header(header);
        let account_id = *header.account_id.as_bytes();

        let register_request = RegisterRequest {
            username: username.to_string(),
            srp_salt: base64_encode(&salt_srp),
            srp_verifier: base64_encode(&verifier),
            srp_group: SRP_GROUP.to_string(),
            kdf_params: kdf_params.to_json(),
            vault_id: Some(crate::codec::hex_encode(header.vault_id.as_bytes())),
            header: Some(base64_encode(&header.to_bytes())),
        };
        let secret_key_string = secret_key.to_emergency_kit(&account_id);
        Ok(Self {
            register_request,
            emergency_kit: EmergencyKit {
                email: username.to_string(),
                server_url: server_url.to_string(),
                secret_key: secret_key_string.clone(),
            },
            setup_code: SetupCode {
                server_url: server_url.to_string(),
                email: username.to_string(),
                secret_key: secret_key_string,
            },
            header: header.clone(),
        })
    }
}

/// Request body for `POST /v1/accounts/register` (mirrors the server DTO).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisterRequest {
    /// Login identifier (email or bare username).
    pub username: String,
    /// Base64 SRP salt.
    pub srp_salt: String,
    /// Base64 SRP verifier.
    pub srp_verifier: String,
    /// SRP group identifier.
    pub srp_group: String,
    /// Opaque KDF parameters; echoed back at `srp/start`.
    pub kdf_params: serde_json::Value,
    /// Hex vault id the header is stored under (issue #129/#131). Optional so
    /// pre-existing clients that don't upload a header still register.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_id: Option<String>,
    /// Base64 non-secret vault header to store at registration, enabling
    /// new-device login and password rotation. Optional (see `vault_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
}

/// Response body for a successful registration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisterResponse {
    /// Server-assigned account id.
    pub account_id: String,
    /// Granted role.
    pub role: String,
    /// Account status.
    pub status: String,
}

/// User-facing recovery sheet. The Secret Key string already embeds the
/// account id and a transcription checksum (`A4-…`).
#[derive(Clone, Debug)]
pub struct EmergencyKit {
    /// Account email / login.
    pub email: String,
    /// Sign-in server URL.
    pub server_url: String,
    /// Emergency-Kit Secret Key string.
    pub secret_key: String,
}

impl EmergencyKit {
    /// Render the kit as plain text (the PDF layout reuses these fields).
    #[must_use]
    pub fn render_text(&self) -> String {
        format!(
            "tock Emergency Kit\n==================\n\nSign-in address : {}\nEmail           : {}\nSecret Key      : {}\n\nKeep this somewhere safe. Your password is NOT stored here.\nYou need both your password AND this Secret Key to sign in on a new device.\n",
            self.server_url, self.email, self.secret_key
        )
    }
}

/// Compact bundle for fast add-device. Encoded as `TOCK1:<base64-json>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupCode {
    /// Sign-in server URL.
    pub server_url: String,
    /// Account email / login.
    pub email: String,
    /// Emergency-Kit Secret Key string.
    pub secret_key: String,
}

const SETUP_PREFIX: &str = "TOCK1:";

impl SetupCode {
    /// Encode as a single scannable/transcribable string.
    #[must_use]
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).unwrap_or_default();
        format!("{SETUP_PREFIX}{}", base64_encode(&json))
    }

    /// Parse a `TOCK1:` Setup Code back into its fields.
    ///
    /// # Errors
    /// Returns [`AccountError::SetupCode`] for a wrong prefix or bad body.
    pub fn parse(s: &str) -> Result<Self, AccountError> {
        let body = s
            .trim()
            .strip_prefix(SETUP_PREFIX)
            .ok_or(AccountError::SetupCode)?;
        let bytes = crate::codec::base64_decode(body).map_err(|_| AccountError::SetupCode)?;
        serde_json::from_slice(&bytes).map_err(|_| AccountError::SetupCode)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::{EmergencyKit, SetupCode, SignupMaterial};

    #[test]
    fn setup_code_round_trips() {
        let c = SetupCode {
            server_url: "https://tock.example".into(),
            email: "a@b.c".into(),
            secret_key: "A4-FOO-BAR".into(),
        };
        assert_eq!(SetupCode::parse(&c.encode()).expect("parse"), c);
    }

    #[test]
    fn new_account_produces_consistent_artifacts() {
        let (m, sk) = SignupMaterial::new_account("a@b.c", "pw", "https://x").expect("new");
        assert_eq!(m.register_request.srp_group, "RFC5054-4096-SHA256");
        assert!(
            m.emergency_kit.secret_key.starts_with("A4-")
                || m.emergency_kit.secret_key.contains('-')
        );
        let parsed = SetupCode::parse(&m.setup_code.encode()).expect("parse");
        assert_eq!(parsed.secret_key, sk);
        assert_eq!(parsed.email, "a@b.c");
    }

    #[test]
    fn setup_code_bad_prefix_rejected() {
        assert!(SetupCode::parse("nope").is_err());
    }

    #[test]
    fn kit_text_has_no_password() {
        let k = EmergencyKit {
            email: "a@b.c".into(),
            server_url: "https://x".into(),
            secret_key: "A4-X".into(),
        };
        let t = k.render_text();
        assert!(t.contains("A4-X"));
        assert!(t.contains("NOT stored"));
    }
}
