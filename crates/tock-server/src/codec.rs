//! Hex / base64 encoding helpers shared by the route and account handlers.
//!
//! The sync API exchanges binary identifiers as hex and opaque blobs as
//! standard base64. These helpers keep that encoding in one place so the
//! vault, account, and admin handlers agree byte-for-byte.

use crate::error::Error;

/// Decode a fixed 16-byte value from a hex string.
pub fn parse_hex_16(s: &str) -> Result<[u8; 16], Error> {
    hex_decode(s)?
        .try_into()
        .map_err(|_| Error::BadRequest("expected 16-byte hex value".into()))
}

/// Decode a fixed 32-byte value from a hex string.
pub fn parse_hex_32(s: &str) -> Result<[u8; 32], Error> {
    hex_decode(s)?
        .try_into()
        .map_err(|_| Error::BadRequest("expected 32-byte hex value".into()))
}

/// Decode an arbitrary-length lowercase/uppercase hex string.
pub fn hex_decode(s: &str) -> Result<Vec<u8>, Error> {
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

/// Decode a standard-alphabet base64 string (padding optional).
pub fn base64_decode(s: &str) -> Result<Vec<u8>, Error> {
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

/// Encode bytes as a standard-alphabet, padded base64 string.
#[must_use]
pub fn base64_encode(bytes: &[u8]) -> String {
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
