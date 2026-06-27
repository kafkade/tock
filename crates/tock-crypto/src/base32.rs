//! Crockford Base32 encoding for human-transcribable secrets.
//!
//! Used by the account [`SecretKey`](crate::secret_key::SecretKey)
//! Emergency-Kit encoding (and reused by higher layers such as the
//! recovery code in `tock-sync`). The alphabet excludes the ambiguous
//! letters `I`, `L`, `O`, `U`; decoding normalizes case and the common
//! `O→0` / `I,L→1` confusions, and skips group separators (`-`, space).

use crate::Error;

/// Crockford Base32 alphabet (excludes `I`, `L`, `O`, `U`).
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Encode bytes as a Crockford Base32 string (no separators).
///
/// Trailing bits that do not fill a full 5-bit group are left-padded
/// with zeros, matching [`decode`]'s truncation behaviour.
#[must_use]
pub fn encode(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    let num_chars = (data.len() * 8).div_ceil(5);
    let mut out = Vec::with_capacity(num_chars);
    let mut buf: u64 = 0;
    let mut bits: u32 = 0;
    for &byte in data {
        buf = (buf << 8) | u64::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buf >> bits) & 0x1F) as usize]);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buf << (5 - bits)) & 0x1F) as usize]);
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Decode a Crockford Base32 string into exactly `expected_bytes`.
///
/// Normalizes lowercase → uppercase and the ambiguous `O → 0`,
/// `I`/`L → 1`; group separators (`-`, space) are skipped. Padding
/// bits beyond `expected_bytes` are discarded.
///
/// # Errors
/// - [`Error::InvalidEncoding`] on an invalid character, or if the
///   input does not carry at least `expected_bytes` bytes of data.
pub fn decode(input: &str, expected_bytes: usize) -> Result<Vec<u8>, Error> {
    let mut values = Vec::new();
    for ch in input.chars() {
        let ch = match ch {
            '-' | ' ' => continue,
            'a'..='z' => ch.to_ascii_uppercase(),
            other => other,
        };
        let ch = match ch {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        let Some(val) = ALPHABET.iter().position(|&c| c == ch as u8) else {
            return Err(Error::InvalidEncoding);
        };
        values.push(u8::try_from(val).unwrap_or(0));
    }

    let mut result = Vec::with_capacity(expected_bytes);
    let mut buf: u64 = 0;
    let mut bits: u32 = 0;
    for val in values {
        buf = (buf << 5) | u64::from(val);
        bits += 5;
        while bits >= 8 {
            bits -= 8;
            result.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    result.truncate(expected_bytes);
    if result.len() != expected_bytes {
        return Err(Error::InvalidEncoding);
    }
    Ok(result)
}

/// Group a bare Crockford string into dash-separated runs of
/// `group_len` characters (e.g. `XXXX-XXXX-XX`).
///
/// `group_len` of `0` returns the input unchanged.
#[must_use]
pub fn group(code: &str, group_len: usize) -> String {
    if group_len == 0 {
        return code.to_string();
    }
    code.as_bytes()
        .chunks(group_len)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{decode, encode, group};

    #[test]
    fn roundtrips_arbitrary_bytes() {
        let data = [0xDE, 0xAD, 0xBE, 0xEF, 0x42];
        let encoded = encode(&data);
        assert_eq!(decode(&encoded, data.len()).expect("decode"), data);
    }

    #[test]
    fn roundtrips_16_bytes() {
        #[allow(clippy::cast_possible_truncation)]
        let data: [u8; 16] = core::array::from_fn(|i| (i as u8).wrapping_mul(17));
        let encoded = encode(&data);
        // 128 bits / 5 = 25.6 → 26 chars.
        assert_eq!(encoded.len(), 26);
        assert_eq!(decode(&encoded, 16).expect("decode"), data);
    }

    #[test]
    fn decode_skips_separators_and_normalizes() {
        let data = [0x00];
        let encoded = encode(&data); // "00"
        let messy = group(&encoded.to_lowercase(), 1); // "0-0"
        assert_eq!(decode(&messy, 1).expect("decode"), data);
    }

    #[test]
    fn decode_normalizes_ambiguous_letters() {
        // O→0, I→1, L→1 should decode like their digit equivalents.
        let canonical = decode("01", 1).expect("decode");
        assert_eq!(decode("OI", 1).expect("decode"), canonical);
        assert_eq!(decode("ol", 1).expect("decode"), canonical);
    }

    #[test]
    fn rejects_invalid_char() {
        assert!(decode("HELLO!", 3).is_err());
    }

    #[test]
    fn rejects_too_short() {
        assert!(decode("00", 8).is_err());
    }

    #[test]
    fn group_inserts_dashes() {
        assert_eq!(group("ABCDEFGHMNPQ", 4), "ABCD-EFGH-MNPQ");
        assert_eq!(group("ABCDEFGHMNPQ", 0), "ABCDEFGHMNPQ");
    }
}
