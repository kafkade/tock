//! User-defined attributes (UDAs) per architecture §3.3.

use std::collections::BTreeMap;

/// Supported UDA value types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UdaType {
    /// Arbitrary UTF-8 text.
    String,
    /// Numeric text interpreted by callers as a number.
    Number,
    /// ISO date text.
    Date,
    /// Boolean value.
    Boolean,
}

impl UdaType {
    /// Return the canonical storage string for this type.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Date => "date",
            Self::Boolean => "boolean",
        }
    }

    /// Parse a type from its canonical storage string.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "string" => Some(Self::String),
            "number" => Some(Self::Number),
            "date" => Some(Self::Date),
            "boolean" | "bool" => Some(Self::Boolean),
            _ => None,
        }
    }
}

/// A declared UDA definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UdaDefinition {
    /// Unique attribute key.
    pub key: String,
    /// Declared attribute type.
    pub uda_type: UdaType,
    /// Optional human-readable label.
    pub label: Option<String>,
    /// Optional default value stored as text.
    pub default: Option<String>,
}

/// UDA values on a task — a thin wrapper over a JSON object.
#[derive(Clone, Debug, Default)]
pub struct UdaValues(
    /// Stored values keyed by UDA name.
    pub BTreeMap<String, serde_json::Value>,
);

impl UdaValues {
    /// Parse from the JSON string stored in the `udas` column.
    #[must_use]
    pub fn from_json(s: &str) -> Self {
        serde_json::from_str::<BTreeMap<String, serde_json::Value>>(s)
            .map(Self)
            .unwrap_or_default()
    }

    /// Serialize to JSON for storage.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.0).unwrap_or_else(|_| String::from("{}"))
    }

    /// Get a UDA value as a string for display or filtering.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<String> {
        self.0.get(key).map(|value| match value {
            serde_json::Value::String(string) => string.clone(),
            other => other.to_string(),
        })
    }

    /// Set a UDA value.
    pub fn set(&mut self, key: &str, value: serde_json::Value) {
        self.0.insert(key.to_string(), value);
    }

    /// Remove a UDA value.
    pub fn remove(&mut self, key: &str) {
        self.0.remove(key);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{UdaType, UdaValues};

    #[test]
    fn parses_type_aliases() {
        assert_eq!(UdaType::from_str_opt("string"), Some(UdaType::String));
        assert_eq!(UdaType::from_str_opt("bool"), Some(UdaType::Boolean));
        assert_eq!(UdaType::from_str_opt("unknown"), None);
    }

    #[test]
    fn parses_invalid_json_as_empty_object() {
        let values = UdaValues::from_json("not-json");
        assert!(values.0.is_empty());
    }

    #[test]
    fn roundtrips_values_to_json() {
        let mut values = UdaValues::default();
        values.set("effort", serde_json::json!(3));
        values.set("owner", serde_json::json!("alex"));

        let parsed = UdaValues::from_json(&values.to_json());
        assert_eq!(parsed.get_str("effort").as_deref(), Some("3"));
        assert_eq!(parsed.get_str("owner").as_deref(), Some("alex"));
    }
}
