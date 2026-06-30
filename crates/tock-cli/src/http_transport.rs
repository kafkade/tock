//! HTTP implementation of the [`tock_sync::transport::Transport`] trait.
//!
//! Per ADR-001 the only Rust crate allowed to perform network I/O is
//! `tock-cli`; the sync *protocol* (wire format, conflict engine) lives
//! in the I/O-free `tock-sync` crate, and this module bridges it to a
//! self-hosted `tock-server` over its JSON+REST API.
//!
//! The server treats every payload as opaque ciphertext: events are
//! wire-encoded single-event frames (`tock_sync::wire::encode_batch`)
//! that are then base64-encoded for JSON transport. Device ids, event
//! ids, and verifying keys travel as lowercase hex. Both encoders are
//! hand-rolled here (matching the server) to avoid pulling extra crates
//! into the dependency tree.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use tock_core::event::{DeviceId, SignedEvent};
use tock_sync::Error;
use tock_sync::transport::{OnboardingBlob, PullBatch, PushAck, SyncCursor, Transport};
use tock_sync::wire;

/// A `Transport` backed by a self-hosted `tock-server`.
pub struct HttpTransport {
    client: Client,
    /// Server base URL, without a trailing slash (e.g. `https://sync.example.com`).
    base: String,
    /// Lowercase-hex 16-byte vault id; selects the server bucket.
    vault_hex: String,
    /// Hex SRP session bearer token, attached as `Authorization: Bearer`.
    bearer: Option<String>,
    /// Hex SRP channel-binding tag, attached as `X-Tock-Channel-Binding`.
    channel_binding: Option<String>,
}

impl HttpTransport {
    /// Build a transport for `vault_id` against `base_url`.
    ///
    /// # Errors
    /// [`Error::Transport`] if the HTTP client cannot be constructed.
    pub fn new(base_url: &str, vault_id: uuid::Uuid) -> Result<Self, Error> {
        let client = Client::builder()
            .build()
            .map_err(|e| Error::Transport(e.to_string()))?;
        Ok(Self {
            client,
            base: base_url.trim_end_matches('/').to_string(),
            vault_hex: hex_encode(vault_id.as_bytes()),
            bearer: None,
            channel_binding: None,
        })
    }

    /// Attach the SRP session bearer token + channel-binding tag (both hex)
    /// so authenticated sync requests are accepted by a self-hosted server.
    #[must_use]
    pub fn with_auth(mut self, bearer: String, channel_binding: String) -> Self {
        self.bearer = Some(bearer);
        self.channel_binding = Some(channel_binding);
        self
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/v1/vaults/{}/{suffix}", self.base, self.vault_hex)
    }

    /// Attach the auth headers (if present) to a request.
    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut rb = rb;
        if let Some(b) = &self.bearer {
            rb = rb.header("authorization", format!("Bearer {b}"));
        }
        if let Some(cb) = &self.channel_binding {
            rb = rb.header("x-tock-channel-binding", cb);
        }
        rb
    }

    /// Upload the (non-secret) vault header so a fresh device can recover the
    /// wrapped Vault Key after SRP login (issue #129). Requires auth.
    ///
    /// # Errors
    /// [`Error::Transport`] on a non-success response.
    pub async fn put_vault_header(&self, header: &[u8]) -> Result<(), Error> {
        #[derive(serde::Serialize)]
        struct HeaderBody {
            header: String,
        }
        let resp = self
            .auth(self.client.put(self.url("header")))
            .json(&HeaderBody {
                header: base64_encode(header),
            })
            .send()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;
        ensure_success(resp).await?;
        Ok(())
    }
}

// ── Server JSON shapes (mirror of `tock_server::routes`) ─────────────

#[derive(serde::Serialize)]
struct RegisterDeviceBody<'a> {
    device_id: String,
    verifying_key: String,
    label: Option<&'a str>,
}

#[derive(serde::Serialize)]
struct PushBody {
    events: Vec<PushItem>,
}

#[derive(serde::Serialize)]
struct PushItem {
    event_id: String,
    device_id: String,
    lamport: i64,
    payload: String,
}

#[derive(Deserialize)]
struct PushResponse {
    accepted: usize,
    duplicates: usize,
    server_lamport: i64,
}

#[derive(Deserialize)]
struct PullResponse {
    events: Vec<PullItem>,
    cursor: i64,
    more: bool,
}

#[derive(Deserialize)]
struct PullItem {
    payload: String,
}

#[derive(serde::Serialize)]
struct BlobBody {
    blob: String,
}

#[derive(Deserialize)]
struct BlobResponse {
    blob: String,
}

#[async_trait]
impl Transport for HttpTransport {
    async fn register_device(
        &self,
        device_id: DeviceId,
        verifying_key: &[u8; 32],
        label: Option<&str>,
    ) -> Result<(), Error> {
        let body = RegisterDeviceBody {
            device_id: hex_encode(device_id.as_bytes()),
            verifying_key: hex_encode(verifying_key),
            label,
        };
        let resp = self
            .auth(self.client.post(self.url("devices")))
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;
        ensure_success(resp).await?;
        Ok(())
    }

    async fn push(&self, events: &[SignedEvent]) -> Result<PushAck, Error> {
        let mut items = Vec::with_capacity(events.len());
        for signed in events {
            // Each event travels as its own single-event wire frame so
            // the server stores self-describing ciphertext blobs.
            let frame = wire::encode_batch(std::slice::from_ref(signed))?;
            items.push(PushItem {
                event_id: hex_encode(signed.event.id.as_bytes()),
                device_id: hex_encode(signed.event.device_id.as_bytes()),
                lamport: i64::try_from(signed.event.lamport).unwrap_or(i64::MAX),
                payload: base64_encode(&frame),
            });
        }
        let resp = self
            .auth(self.client.post(self.url("events/push")))
            .json(&PushBody { events: items })
            .send()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;
        let resp = ensure_success(resp).await?;
        let parsed: PushResponse = resp
            .json()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;
        Ok(PushAck {
            accepted: parsed.accepted,
            duplicates: parsed.duplicates,
            server_lamport: u64::try_from(parsed.server_lamport).unwrap_or(0),
        })
    }

    async fn pull(&self, cursor: SyncCursor, limit: usize) -> Result<PullBatch, Error> {
        let after = i64::try_from(cursor.position).unwrap_or(i64::MAX);
        let resp = self
            .auth(self.client.get(self.url("events/pull")))
            .query(&[("after", after.to_string()), ("limit", limit.to_string())])
            .send()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;
        let resp = ensure_success(resp).await?;
        let parsed: PullResponse = resp
            .json()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;
        let mut events = Vec::with_capacity(parsed.events.len());
        for item in &parsed.events {
            let frame = base64_decode(&item.payload)
                .map_err(|()| Error::WireFormat("invalid base64 payload"))?;
            let mut batch = wire::decode_batch(&frame)?;
            events.append(&mut batch);
        }
        let next = u64::try_from(parsed.cursor).unwrap_or(cursor.position);
        Ok(PullBatch {
            events,
            next_cursor: SyncCursor::at(next),
            more: parsed.more,
        })
    }

    async fn put_onboarding_blob(
        &self,
        target_device: DeviceId,
        blob: OnboardingBlob,
    ) -> Result<(), Error> {
        let body = BlobBody {
            blob: base64_encode(&blob.encode()),
        };
        let resp = self
            .auth(self.client.put(self.url(&format!(
                "onboarding/{}",
                hex_encode(target_device.as_bytes())
            ))))
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;
        ensure_success(resp).await?;
        Ok(())
    }

    async fn get_onboarding_blob(&self, device: DeviceId) -> Result<Option<OnboardingBlob>, Error> {
        let resp = self
            .auth(
                self.client
                    .get(self.url(&format!("onboarding/{}", hex_encode(device.as_bytes())))),
            )
            .send()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let resp = ensure_success(resp).await?;
        let parsed: BlobResponse = resp
            .json()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;
        let raw =
            base64_decode(&parsed.blob).map_err(|()| Error::WireFormat("invalid base64 blob"))?;
        Ok(Some(OnboardingBlob::decode(&raw)?))
    }
}

/// Map a non-2xx response to a transport error, consuming the body.
async fn ensure_success(resp: reqwest::Response) -> Result<reqwest::Response, Error> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    Err(Error::Transport(format!(
        "server returned {status}: {body}"
    )))
}

// ── Hand-rolled hex / base64 (standard alphabet, matches the server) ──

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map_or(0, |b| u32::from(*b));
        let b2 = chunk.get(2).map_or(0, |b| u32::from(*b));
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

fn base64_decode(s: &str) -> Result<Vec<u8>, ()> {
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
            #[allow(clippy::cast_possible_truncation)]
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::{base64_decode, base64_encode, hex_encode};

    #[test]
    fn hex_roundtrip_known() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0x10]), "00ff10");
    }

    #[test]
    fn base64_roundtrip() {
        for sample in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            let encoded = base64_encode(sample);
            let decoded = base64_decode(&encoded).expect("decode");
            assert_eq!(decoded, sample);
        }
    }
}
