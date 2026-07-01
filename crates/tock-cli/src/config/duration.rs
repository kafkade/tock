//! Duration parsing helpers for config values.
//!
//! Focus intervals accept either a bare integer (minutes) or a duration
//! string such as `"25m"`, `"1h"`, or `"90s"` (rounded to whole minutes).

/// Parse a duration expressed in minutes from an integer or a suffixed string.
///
/// Accepts `"25m"`, `"1h"`, `"90s"` (rounded up to whole minutes), or a bare
/// number (interpreted as minutes). Returns `None` on malformed input.
#[must_use]
pub fn parse_minutes(raw: &str) -> Option<u32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = trimmed.parse::<u32>() {
        return Some(value);
    }
    let (number, unit) = trimmed.split_at(trimmed.len() - 1);
    let value = number.trim().parse::<u32>().ok()?;
    match unit {
        "m" | "M" => Some(value),
        "h" | "H" => Some(value.saturating_mul(60)),
        "s" | "S" => Some(value.div_ceil(60)),
        _ => None,
    }
}

/// Serde adapter that reads minutes from an integer or a duration string and
/// writes them back as an integer.
pub mod minutes {
    use serde::{Deserialize, Deserializer, Serializer};

    use super::parse_minutes;

    /// Deserialize a minute count from a TOML integer or duration string.
    ///
    /// # Errors
    /// Returns a deserialization error for malformed durations.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<u32, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Int(u32),
            Str(String),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Int(value) => Ok(value),
            Repr::Str(text) => parse_minutes(&text)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid duration: {text:?}"))),
        }
    }

    /// Serialize a minute count as a plain integer.
    ///
    /// # Errors
    /// Propagates serializer failures.
    #[allow(clippy::trivially_copy_pass_by_ref)] // serde `with` signature is fixed
    pub fn serialize<S>(value: &u32, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(*value)
    }
}

#[cfg(test)]
mod tests {
    use super::parse_minutes;

    #[test]
    fn parses_bare_integer_as_minutes() {
        assert_eq!(parse_minutes("25"), Some(25));
    }

    #[test]
    fn parses_suffixed_durations() {
        assert_eq!(parse_minutes("25m"), Some(25));
        assert_eq!(parse_minutes("1h"), Some(60));
        assert_eq!(parse_minutes("90s"), Some(2));
        assert_eq!(parse_minutes("30s"), Some(1));
    }

    #[test]
    fn rejects_malformed() {
        assert_eq!(parse_minutes(""), None);
        assert_eq!(parse_minutes("abc"), None);
        assert_eq!(parse_minutes("5x"), None);
    }
}
