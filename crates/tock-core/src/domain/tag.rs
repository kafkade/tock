//! Tag domain model per architecture §2.1 and issue #12.
//!
//! Tags are N:N associations between entities (tasks, projects,
//! habits, time blocks) via the `entity_tags` join table.
//!
//! ## Sigil syntax
//!
//! The CLI parser extracts tags from input text using sigils:
//! - `#tag` — adds the tag `tag`
//! - `-#tag` — removes the tag `tag`
//!
//! Priority and dates use different sigils and are handled in the
//! CLI input parser, not here.

use uuid::Uuid;

/// A tag — a reusable label applied to entities.
#[derive(Clone, Debug)]
pub struct Tag {
    /// Globally unique identifier.
    pub id: Uuid,
    /// Tag name (e.g. `errands`, `home/repairs`).
    pub name: String,
    /// Display color.
    pub color: Option<String>,
}

/// Parse tags from an input string. Returns `(clean_text, tags_to_add,
/// tags_to_remove)`.
///
/// Recognized patterns:
/// - `#tag_name` → add tag
/// - `-#tag_name` → remove tag
///
/// Everything else remains in `clean_text`.
#[must_use]
pub fn parse_sigils(input: &str) -> (String, Vec<String>, Vec<String>) {
    let mut clean = Vec::new();
    let mut add = Vec::new();
    let mut remove = Vec::new();

    for token in input.split_whitespace() {
        if let Some(tag) = token.strip_prefix("-#") {
            if !tag.is_empty() {
                remove.push(tag.to_string());
            }
        } else if let Some(tag) = token.strip_prefix('#') {
            if !tag.is_empty() {
                add.push(tag.to_string());
            }
        } else {
            clean.push(token);
        }
    }

    (clean.join(" "), add, remove)
}

/// Parse priority from input tokens. Recognizes `!H`, `!M`, `!L`.
/// Returns `(remaining_text, priority_char_if_found)`.
#[must_use]
pub fn parse_priority(input: &str) -> (String, Option<char>) {
    let mut clean = Vec::new();
    let mut priority = None;

    for token in input.split_whitespace() {
        match token {
            "!H" | "!h" => priority = Some('H'),
            "!M" | "!m" => priority = Some('M'),
            "!L" | "!l" => priority = Some('L'),
            _ => clean.push(token),
        }
    }

    (clean.join(" "), priority)
}

/// Parse a `due:YYYY-MM-DD` token from input.
/// Returns `(remaining_text, deadline_if_found)`.
#[must_use]
pub fn parse_deadline(input: &str) -> (String, Option<String>) {
    let mut clean = Vec::new();
    let mut deadline = None;

    for token in input.split_whitespace() {
        if let Some(date) = token.strip_prefix("due:") {
            if !date.is_empty() {
                deadline = Some(date.to_string());
            }
        } else {
            clean.push(token);
        }
    }

    (clean.join(" "), deadline)
}

/// Parse a `sched:`/`scheduled:` token from input.
/// Returns `(remaining_text, scheduled_if_found)`.
///
/// The value is a single whitespace-free token, e.g. `sched:2026-06-01`,
/// `sched:2026-06-01T14:00`, or `sched:tomorrow`. Multi-word natural-language
/// slots are handled by the `schedule` command, which joins its arguments.
#[must_use]
pub fn parse_scheduled(input: &str) -> (String, Option<String>) {
    let mut clean = Vec::new();
    let mut scheduled = None;

    for token in input.split_whitespace() {
        let value = token
            .strip_prefix("scheduled:")
            .or_else(|| token.strip_prefix("sched:"));
        if let Some(value) = value {
            if !value.is_empty() {
                scheduled = Some(value.to_string());
            }
        } else {
            clean.push(token);
        }
    }

    (clean.join(" "), scheduled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sigils_extracts_tags() {
        let (text, add, remove) = parse_sigils("Buy groceries #errands #shopping");
        assert_eq!(text, "Buy groceries");
        assert_eq!(add, vec!["errands", "shopping"]);
        assert!(remove.is_empty());
    }

    #[test]
    fn parse_sigils_handles_removal() {
        let (text, add, remove) = parse_sigils("Fix bug -#wontfix #urgent");
        assert_eq!(text, "Fix bug");
        assert_eq!(add, vec!["urgent"]);
        assert_eq!(remove, vec!["wontfix"]);
    }

    #[test]
    fn parse_priority_extracts() {
        let (text, p) = parse_priority("Important task !H");
        assert_eq!(text, "Important task");
        assert_eq!(p, Some('H'));
    }

    #[test]
    fn parse_deadline_extracts() {
        let (text, d) = parse_deadline("Submit report due:2026-05-25");
        assert_eq!(text, "Submit report");
        assert_eq!(d, Some("2026-05-25".to_string()));
    }

    #[test]
    fn parse_scheduled_extracts_sched_alias() {
        let (text, s) = parse_scheduled("Draft memo sched:2026-05-25");
        assert_eq!(text, "Draft memo");
        assert_eq!(s, Some("2026-05-25".to_string()));
    }

    #[test]
    fn parse_scheduled_extracts_full_prefix_and_time() {
        let (text, s) = parse_scheduled("Standup scheduled:2026-05-25T09:30");
        assert_eq!(text, "Standup");
        assert_eq!(s, Some("2026-05-25T09:30".to_string()));
    }

    #[test]
    fn parse_scheduled_absent() {
        let (text, s) = parse_scheduled("No slot here");
        assert_eq!(text, "No slot here");
        assert_eq!(s, None);
    }

    #[test]
    fn parse_combined() {
        let input = "Buy groceries #errands !H due:2026-06-01";
        let (text1, tags, _) = parse_sigils(input);
        let (text2, prio) = parse_priority(&text1);
        let (final_text, deadline) = parse_deadline(&text2);
        assert_eq!(final_text, "Buy groceries");
        assert_eq!(tags, vec!["errands"]);
        assert_eq!(prio, Some('H'));
        assert_eq!(deadline, Some("2026-06-01".to_string()));
    }
}
