//! Minimal iCalendar (RFC 5545) parser and serializer.
//!
//! Supports the subset needed for `CalDAV` task/event sync:
//! VCALENDAR, VTODO, VEVENT wrappers and common properties.
//! Handles line folding/unfolding per RFC 5545 §3.1.

use crate::Error;

/// A parsed iCalendar property (name + optional params + value).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Property {
    /// Property name (e.g. `SUMMARY`, `DUE`, `X-APP-PROJECT`).
    pub name: String,
    /// Optional parameters (e.g. `TZID=America/New_York`).
    pub params: Vec<(String, String)>,
    /// Property value.
    pub value: String,
}

impl Property {
    /// Create a simple property with no parameters.
    #[must_use]
    pub fn new(name: &str, value: &str) -> Self {
        Self {
            name: name.to_uppercase(),
            params: Vec::new(),
            value: value.to_string(),
        }
    }

    /// Create a property with parameters.
    #[must_use]
    pub fn with_params(name: &str, params: Vec<(String, String)>, value: &str) -> Self {
        Self {
            name: name.to_uppercase(),
            params,
            value: value.to_string(),
        }
    }

    /// Get a parameter value by name (case-insensitive).
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        let upper = name.to_uppercase();
        self.params
            .iter()
            .find(|(k, _)| k.to_uppercase() == upper)
            .map(|(_, v)| v.as_str())
    }

    /// Serialize this property to iCalendar format (with line folding).
    #[must_use]
    pub fn to_ical(&self) -> String {
        let mut line = self.name.clone();
        for (k, v) in &self.params {
            line.push(';');
            line.push_str(k);
            line.push('=');
            // Quote values containing special characters.
            if v.contains([',', ';', ':']) {
                line.push('"');
                line.push_str(v);
                line.push('"');
            } else {
                line.push_str(v);
            }
        }
        line.push(':');
        line.push_str(&self.value);
        fold_line(&line)
    }
}

/// A parsed iCalendar component (VCALENDAR, VTODO, VEVENT, etc.).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    /// Component type (e.g. `VCALENDAR`, `VTODO`, `VEVENT`).
    pub kind: String,
    /// Properties of this component.
    pub properties: Vec<Property>,
    /// Nested sub-components.
    pub children: Vec<Self>,
}

impl Component {
    /// Create a new empty component.
    #[must_use]
    pub fn new(kind: &str) -> Self {
        Self {
            kind: kind.to_uppercase(),
            properties: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Get the first property value by name (case-insensitive).
    #[must_use]
    pub fn prop_value(&self, name: &str) -> Option<&str> {
        let upper = name.to_uppercase();
        self.properties
            .iter()
            .find(|p| p.name == upper)
            .map(|p| p.value.as_str())
    }

    /// Get the first property by name (case-insensitive).
    #[must_use]
    pub fn prop(&self, name: &str) -> Option<&Property> {
        let upper = name.to_uppercase();
        self.properties.iter().find(|p| p.name == upper)
    }

    /// Get all properties with the given name (case-insensitive).
    #[must_use]
    pub fn props(&self, name: &str) -> Vec<&Property> {
        let upper = name.to_uppercase();
        self.properties.iter().filter(|p| p.name == upper).collect()
    }

    /// Get all properties whose name starts with a prefix.
    #[must_use]
    pub fn props_prefixed(&self, prefix: &str) -> Vec<&Property> {
        let upper = prefix.to_uppercase();
        self.properties
            .iter()
            .filter(|p| p.name.starts_with(&upper))
            .collect()
    }

    /// Add a property.
    pub fn add_prop(&mut self, prop: Property) {
        self.properties.push(prop);
    }

    /// Set a property (replace first existing or add).
    pub fn set_prop(&mut self, prop: Property) {
        if let Some(existing) = self.properties.iter_mut().find(|p| p.name == prop.name) {
            *existing = prop;
        } else {
            self.properties.push(prop);
        }
    }

    /// Remove all properties with the given name.
    pub fn remove_props(&mut self, name: &str) {
        let upper = name.to_uppercase();
        self.properties.retain(|p| p.name != upper);
    }

    /// Get the first child of a given kind.
    #[must_use]
    pub fn child(&self, kind: &str) -> Option<&Self> {
        let upper = kind.to_uppercase();
        self.children.iter().find(|c| c.kind == upper)
    }

    /// Get all children of a given kind.
    #[must_use]
    pub fn children_of(&self, kind: &str) -> Vec<&Self> {
        let upper = kind.to_uppercase();
        self.children.iter().filter(|c| c.kind == upper).collect()
    }

    /// Serialize this component to iCalendar format.
    #[must_use]
    pub fn to_ical(&self) -> String {
        let mut out = String::new();
        out.push_str("BEGIN:");
        out.push_str(&self.kind);
        out.push_str("\r\n");
        for prop in &self.properties {
            out.push_str(&prop.to_ical());
            out.push_str("\r\n");
        }
        for child in &self.children {
            out.push_str(&child.to_ical());
        }
        out.push_str("END:");
        out.push_str(&self.kind);
        out.push_str("\r\n");
        out
    }
}

/// Unfold iCalendar content lines per RFC 5545 §3.1.
///
/// Lines that begin with a space or tab are continuations of the
/// previous line.
#[must_use]
pub fn unfold(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut result = String::with_capacity(normalized.len());
    for line in normalized.split_terminator('\n') {
        if line.starts_with([' ', '\t']) {
            // Continuation: strip the leading whitespace and append.
            result.push_str(&line[1..]);
        } else {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line);
        }
    }
    result
}

/// Fold a content line to 75 octets per RFC 5545 §3.1.
#[must_use]
pub fn fold_line(line: &str) -> String {
    const MAX_LINE: usize = 75;
    if line.len() <= MAX_LINE {
        return line.to_string();
    }
    let mut result = String::with_capacity(line.len() + line.len() / MAX_LINE * 3);
    let mut remaining = line;
    let mut first = true;
    while !remaining.is_empty() {
        let limit = if first { MAX_LINE } else { MAX_LINE - 1 };
        // Find a safe split point that doesn't break a multi-byte char.
        let split_at = if remaining.len() <= limit {
            remaining.len()
        } else {
            let mut pos = limit;
            while pos > 0 && !remaining.is_char_boundary(pos) {
                pos -= 1;
            }
            if pos == 0 { limit } else { pos }
        };
        if !first {
            result.push_str("\r\n ");
        }
        result.push_str(&remaining[..split_at]);
        remaining = &remaining[split_at..];
        first = false;
    }
    result
}

/// Parse an iCalendar document into a [`Component`] tree.
///
/// # Errors
/// Returns [`Error::IcalParse`] if the input is malformed.
pub fn parse(input: &str) -> Result<Component, Error> {
    let unfolded = unfold(input);
    let lines: Vec<&str> = unfolded.lines().filter(|l| !l.is_empty()).collect();
    let (component, _) = parse_component(&lines, 0)?;
    Ok(component)
}

fn parse_component(lines: &[&str], start: usize) -> Result<(Component, usize), Error> {
    let first = lines
        .get(start)
        .ok_or_else(|| Error::IcalParse("unexpected end of input".into()))?;
    let kind = first
        .strip_prefix("BEGIN:")
        .ok_or_else(|| Error::IcalParse(format!("expected BEGIN:, got: {first}")))?;
    let mut component = Component::new(kind);
    let mut i = start + 1;

    while i < lines.len() {
        let line = lines[i];
        if let Some(end_kind) = line.strip_prefix("END:") {
            if !end_kind.eq_ignore_ascii_case(&component.kind) {
                return Err(Error::IcalParse(format!(
                    "mismatched END: expected {}, got {end_kind}",
                    component.kind
                )));
            }
            return Ok((component, i + 1));
        }
        if line.starts_with("BEGIN:") {
            let (child, next_i) = parse_component(lines, i)?;
            component.children.push(child);
            i = next_i;
        } else {
            component.properties.push(parse_property(line)?);
            i += 1;
        }
    }

    Err(Error::IcalParse(format!("missing END:{}", component.kind)))
}

fn parse_property(line: &str) -> Result<Property, Error> {
    // Split on first unquoted colon.
    let (name_params, value) = split_property_line(line)?;

    // Split name from params on first semicolon.
    let mut parts = name_params.splitn(2, ';');
    let name = parts
        .next()
        .ok_or_else(|| Error::IcalParse("empty property name".into()))?
        .to_uppercase();
    let params = parts.next().map_or_else(Vec::new, parse_params);

    Ok(Property {
        name,
        params,
        value: value.to_string(),
    })
}

/// Split a property line into (name+params, value) at the first
/// unquoted colon.
fn split_property_line(line: &str) -> Result<(&str, &str), Error> {
    let mut in_quotes = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                return Ok((&line[..i], &line[i + 1..]));
            }
            _ => {}
        }
    }
    Err(Error::IcalParse(format!(
        "no colon in property line: {line}"
    )))
}

/// Parse parameter string like `TZID=America/New_York;VALUE=DATE`.
fn parse_params(input: &str) -> Vec<(String, String)> {
    let mut params = Vec::new();
    let mut remaining = input;

    while !remaining.is_empty() {
        // Find `=` separator.
        let Some(eq_pos) = remaining.find('=') else {
            break;
        };
        let key = remaining[..eq_pos].to_uppercase();
        remaining = &remaining[eq_pos + 1..];

        // Value may be quoted.
        if remaining.starts_with('"') {
            // Find closing quote.
            let end = remaining[1..].find('"').map_or(remaining.len(), |p| p + 2);
            let val = &remaining[1..end - 1];
            params.push((key, val.to_string()));
            remaining = if end < remaining.len() && remaining.as_bytes().get(end) == Some(&b';') {
                &remaining[end + 1..]
            } else {
                &remaining[end..]
            };
        } else {
            // Unquoted — find next `;` or end.
            let end = remaining.find(';').unwrap_or(remaining.len());
            let val = &remaining[..end];
            params.push((key, val.to_string()));
            remaining = if end < remaining.len() {
                &remaining[end + 1..]
            } else {
                ""
            };
        }
    }
    params
}

/// Escape a text value per RFC 5545 §3.3.11.
#[must_use]
pub fn escape_text(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

/// Unescape a text value per RFC 5545 §3.3.11.
#[must_use]
pub fn unescape_text(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n' | 'N') => result.push('\n'),
                Some(c) => result.push(c),
                None => result.push('\\'),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn unfold_simple() {
        let input = "SUMMARY:This is a\r\n  long line\r\n";
        assert_eq!(unfold(input), "SUMMARY:This is a long line");
    }

    #[test]
    fn fold_short_line() {
        assert_eq!(fold_line("SUMMARY:Short"), "SUMMARY:Short");
    }

    #[test]
    fn fold_long_line() {
        let long = format!("DESCRIPTION:{}", "x".repeat(100));
        let folded = fold_line(&long);
        assert!(folded.contains("\r\n "));
        // Unfolding the folded result should give back the original.
        let unfolded = unfold(&folded);
        assert_eq!(unfolded, long);
    }

    #[test]
    fn parse_simple_vtodo() {
        let ical = "\
BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//tock//EN\r\n\
BEGIN:VTODO\r\n\
UID:abc-123\r\n\
SUMMARY:Buy milk\r\n\
PRIORITY:5\r\n\
STATUS:NEEDS-ACTION\r\n\
END:VTODO\r\n\
END:VCALENDAR\r\n";
        let cal = parse(ical).expect("parse failed");
        assert_eq!(cal.kind, "VCALENDAR");
        assert_eq!(cal.prop_value("VERSION"), Some("2.0"));
        let todo = cal.child("VTODO").expect("no VTODO");
        assert_eq!(todo.prop_value("UID"), Some("abc-123"));
        assert_eq!(todo.prop_value("SUMMARY"), Some("Buy milk"));
        assert_eq!(todo.prop_value("PRIORITY"), Some("5"));
    }

    #[test]
    fn parse_vevent() {
        let ical = "\
BEGIN:VCALENDAR\r\n\
BEGIN:VEVENT\r\n\
UID:evt-1\r\n\
SUMMARY:Deep work\r\n\
DTSTART:20260531T090000Z\r\n\
DTEND:20260531T110000Z\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";
        let cal = parse(ical).expect("parse");
        let evt = cal.child("VEVENT").expect("no VEVENT");
        assert_eq!(evt.prop_value("DTSTART"), Some("20260531T090000Z"));
        assert_eq!(evt.prop_value("DTEND"), Some("20260531T110000Z"));
    }

    #[test]
    fn parse_property_with_params() {
        let prop = parse_property("DUE;VALUE=DATE:20260601").expect("parse");
        assert_eq!(prop.name, "DUE");
        assert_eq!(prop.param("VALUE"), Some("DATE"));
        assert_eq!(prop.value, "20260601");
    }

    #[test]
    fn roundtrip_vtodo() {
        let mut cal = Component::new("VCALENDAR");
        cal.add_prop(Property::new("VERSION", "2.0"));
        cal.add_prop(Property::new("PRODID", "-//tock//EN"));
        let mut todo = Component::new("VTODO");
        todo.add_prop(Property::new("UID", "test-uid"));
        todo.add_prop(Property::new("SUMMARY", "Test task"));
        todo.add_prop(Property::new("PRIORITY", "1"));
        cal.children.push(todo);

        let serialized = cal.to_ical();
        let parsed = parse(&serialized).expect("roundtrip parse");
        assert_eq!(parsed.kind, "VCALENDAR");
        let t = parsed.child("VTODO").expect("VTODO");
        assert_eq!(t.prop_value("SUMMARY"), Some("Test task"));
        assert_eq!(t.prop_value("PRIORITY"), Some("1"));
    }

    #[test]
    fn escape_unescape_roundtrip() {
        let original = "Line 1\nLine 2; with comma, and backslash\\";
        let escaped = escape_text(original);
        let unescaped = unescape_text(&escaped);
        assert_eq!(unescaped, original);
    }

    #[test]
    fn parse_categories() {
        let prop = parse_property("CATEGORIES:work,urgent,home").expect("parse");
        assert_eq!(prop.name, "CATEGORIES");
        assert_eq!(prop.value, "work,urgent,home");
    }

    #[test]
    fn parse_quoted_param() {
        let prop =
            parse_property("ATTENDEE;CN=\"John Doe\":mailto:john@example.com").expect("parse");
        assert_eq!(prop.param("CN"), Some("John Doe"));
        assert_eq!(prop.value, "mailto:john@example.com");
    }

    #[test]
    fn component_set_prop_replaces() {
        let mut c = Component::new("VTODO");
        c.add_prop(Property::new("SUMMARY", "old"));
        c.set_prop(Property::new("SUMMARY", "new"));
        assert_eq!(c.prop_value("SUMMARY"), Some("new"));
        assert_eq!(c.props("SUMMARY").len(), 1);
    }

    #[test]
    fn x_app_props_roundtrip() {
        let mut todo = Component::new("VTODO");
        todo.add_prop(Property::new("X-APP-PROJECT", "Work"));
        todo.add_prop(Property::new("X-APP-UDA-EFFORT", "5"));
        let serialized = todo.to_ical();
        let parsed =
            parse(&format!("BEGIN:VCALENDAR\r\n{serialized}END:VCALENDAR\r\n")).expect("parse");
        let t = parsed.child("VTODO").expect("VTODO");
        assert_eq!(t.prop_value("X-APP-PROJECT"), Some("Work"));
        assert_eq!(t.prop_value("X-APP-UDA-EFFORT"), Some("5"));
        assert_eq!(t.props_prefixed("X-APP-").len(), 2);
    }
}
