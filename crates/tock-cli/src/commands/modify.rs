//! `tock mod` — modify an existing task.

use tock_core::domain::tag::{parse_deadline, parse_priority, parse_scheduled, parse_sigils};
use tock_core::domain::task::{Priority, TaskPatch};

use super::add::resolve_scheduled;

/// Parse modification arguments into a `TaskPatch`.
#[must_use]
pub fn parse_modify_args(args: &[String]) -> TaskPatch {
    let raw = args.join(" ");
    let (text, add_tags, remove_tags) = parse_sigils(&raw);
    let (text, prio_char) = parse_priority(&text);
    let (text, scheduled) = parse_scheduled(&text);
    let (remaining, deadline) = parse_deadline(&text);

    let mut patch = TaskPatch {
        add_tags,
        remove_tags,
        ..TaskPatch::default()
    };

    if let Some(c) = prio_char {
        patch.priority = Some(Priority::from_str_opt(&String::from(c)));
    }
    if let Some(d) = deadline {
        patch.deadline = Some(Some(d));
    }
    if let Some(s) = scheduled {
        // Resolve through the NL date/time parser, falling back to the raw
        // token if it can't be understood. Clearing a slot is done via the
        // dedicated `unschedule` command.
        let resolved = resolve_scheduled(&s).unwrap_or(s);
        patch.scheduled_for = Some(Some(resolved));
    }

    if let Some(title_val) = extract_field(&remaining, "title") {
        patch.title = Some(title_val);
    }
    if let Some(notes_val) = extract_field(&remaining, "notes") {
        patch.notes = Some(Some(notes_val));
    }

    for token in remaining.split_whitespace() {
        if let Some(rest) = token.strip_prefix("uda.")
            && let Some((key, value)) = rest.split_once(':')
        {
            patch.set_udas.insert(
                key.to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    patch
}

fn extract_field(text: &str, field: &str) -> Option<String> {
    let prefix = format!("{field}:");
    for token in text.split_whitespace() {
        if let Some(val) = token.strip_prefix(&prefix) {
            return Some(val.trim_matches('"').to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::parse_modify_args;

    #[test]
    fn parses_uda_assignments() {
        let args = vec![String::from("uda.owner:sam"), String::from("uda.effort:5")];
        let patch = parse_modify_args(&args);

        assert_eq!(patch.set_udas["owner"], serde_json::json!("sam"));
        assert_eq!(patch.set_udas["effort"], serde_json::json!("5"));
    }

    #[test]
    fn parses_scheduled_sigil() {
        let args = vec![String::from("sched:2026-06-01T09:00")];
        let patch = parse_modify_args(&args);

        assert_eq!(
            patch.scheduled_for,
            Some(Some("2026-06-01T09:00".to_string()))
        );
    }
}
