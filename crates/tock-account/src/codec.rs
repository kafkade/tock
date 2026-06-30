//! Standard-alphabet base64 and lowercase hex, dependency-free.
//!
//! The wire protocol exchanges binary identifiers as hex and opaque blobs as
//! padded base64. These helpers match `tock-server`'s `codec` module
//! byte-for-byte so the shared orchestration layer encodes exactly what the
//! server decodes.

use crate::error::AccountError;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes as a standard-alphabet, padded base64 string.
#[must_use]
pub fn base64_encode(bytes: &[u8]) -> String {
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
        out.push(if chunk.len() > 1 {
            ALPHABET[((triple >> 6) & 0x3F) as usize]
        } else {
            b'='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(triple & 0x3F) as usize]
        } else {
            b'='
        });
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Decode a standard-alphabet base64 string (padding optional).
///
/// # Errors
/// Returns [`AccountError::Encoding`] on any non-alphabet character.
pub fn base64_decode(s: &str) -> Result<Vec<u8>, AccountError> {
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
            _ => return Err(AccountError::Encoding("base64")),
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

/// Encode bytes as a lowercase hex string.
#[must_use]
pub fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

/// Decode an arbitrary-length hex string.
///
/// # Errors
/// Returns [`AccountError::Encoding`] on odd length or non-hex characters.
pub fn hex_decode(s: &str) -> Result<Vec<u8>, AccountError> {
    if !s.len().is_multiple_of(2) {
        return Err(AccountError::Encoding("hex length"));
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().chunks(2) {
        let hi = nibble(chunk[0])?;
        let lo = nibble(chunk[1])?;
        bytes.push((hi << 4) | lo);
    }
    Ok(bytes)
}

const fn nibble(b: u8) -> Result<u8, AccountError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(AccountError::Encoding("hex char")),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::{base64_decode, base64_encode, hex_decode, hex_encode};

    #[test]
    fn base64_round_trips() {
        for v in [
            vec![],
            vec![0],
            vec![1, 2, 3],
            (0u8..=255).collect::<Vec<_>>(),
        ] {
            assert_eq!(base64_decode(&base64_encode(&v)).expect("decode"), v);
        }
    }

    #[test]
    fn hex_round_trips() {
        let v = (0u8..=255).collect::<Vec<_>>();
        assert_eq!(hex_decode(&hex_encode(&v)).expect("decode"), v);
    }
}
