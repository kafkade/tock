//! `tock mod` — modify an existing task.

use tock_core::domain::tag::{parse_deadline, parse_priority, parse_sigils};
use tock_core::domain::task::{Priority, TaskPatch};

/// Parse modification arguments into a `TaskPatch`.
#[must_use]
pub fn parse_modify_args(args: &[String]) -> TaskPatch {
    let raw = args.join(" ");
    let (text, add_tags, remove_tags) = parse_sigils(&raw);
    let (text, prio_char) = parse_priority(&text);
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

    // Check for title:"..." syntax.
    if let Some(title_val) = extract_field(&remaining, "title") {
        patch.title = Some(title_val);
    }
    if let Some(notes_val) = extract_field(&remaining, "notes") {
        patch.notes = Some(Some(notes_val));
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
