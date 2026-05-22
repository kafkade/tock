//! `tock add` — create a new task.

use tock_core::domain::recurrence::{RecurrenceMode, RecurrencePattern, RecurrenceSpec};
use tock_core::domain::tag::{parse_deadline, parse_priority, parse_sigils};
use tock_core::domain::task::{NewTask, Priority};

/// Parse the input words into a `NewTask`.
///
/// Supports sigil extraction (`#tag`, `!H/M/L`, `due:...`) and
/// natural-language date parsing for the deadline.
#[must_use]
pub fn parse_add_input(words: &[String]) -> NewTask {
    let raw = words.join(" ");
    let (text, tags, _remove) = parse_sigils(&raw);
    let (text, prio_char) = parse_priority(&text);
    let (title, deadline_raw) = parse_deadline(&text);

    let priority = prio_char.and_then(|c| Priority::from_str_opt(&String::from(c)));

    // Try NL date parsing on the deadline value.
    let deadline = deadline_raw.and_then(|d| resolve_deadline(&d).or(Some(d)));

    NewTask {
        title,
        tags,
        priority,
        deadline,
        ..NewTask::default()
    }
}

/// Parse the `--recur` flag into stored JSON.
///
/// # Errors
/// Returns an error message when the recurrence string is invalid.
pub fn parse_recur_flag(raw: Option<&str>) -> Result<Option<String>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let normalized = raw.trim().to_ascii_lowercase();
    let pattern = match normalized.as_str() {
        "daily" => RecurrencePattern::Daily,
        "weekly" => RecurrencePattern::Weekly,
        "monthly" => RecurrencePattern::Monthly,
        "yearly" => RecurrencePattern::Yearly,
        _ => parse_interval_pattern(&normalized)?,
    };
    Ok(Some(
        RecurrenceSpec {
            pattern,
            mode: RecurrenceMode::Periodic,
        }
        .to_json(),
    ))
}

/// Resolve a deadline string through the NL date parser.
/// Returns `Some("YYYY-MM-DD")` or `None` if unrecognized.
fn resolve_deadline(input: &str) -> Option<String> {
    let today = time::OffsetDateTime::now_utc().date();
    let d = tock_parse::date::parse_date(input, today)?;
    Some(format!(
        "{:04}-{:02}-{:02}",
        d.year(),
        u8::from(d.month()),
        d.day()
    ))
}

fn parse_interval_pattern(raw: &str) -> Result<RecurrencePattern, String> {
    let Some(rest) = raw.strip_prefix("every-") else {
        return Err(String::from(
            "invalid recurrence; use daily, weekly, monthly, yearly, every-3d, or every-2w",
        ));
    };

    let (number, suffix) = rest.split_at(rest.len().saturating_sub(1));
    let interval = number
        .parse::<u32>()
        .map_err(|_| String::from("invalid recurrence interval"))?;
    if interval == 0 {
        return Err(String::from(
            "recurrence interval must be greater than zero",
        ));
    }

    match suffix {
        "d" => Ok(RecurrencePattern::EveryNDays(interval)),
        "w" => Ok(RecurrencePattern::EveryNWeeks(interval)),
        _ => Err(String::from(
            "invalid recurrence; use daily, weekly, monthly, yearly, every-3d, or every-2w",
        )),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn parse_simple_task() {
        let words: Vec<String> = "Buy groceries"
            .split_whitespace()
            .map(String::from)
            .collect();
        let t = parse_add_input(&words);
        assert_eq!(t.title, "Buy groceries");
        assert!(t.tags.is_empty());
        assert!(t.priority.is_none());
        assert!(t.deadline.is_none());
    }

    #[test]
    fn parse_task_with_sigils() {
        let words: Vec<String> = "Buy groceries #errands !H due:2026-06-01"
            .split_whitespace()
            .map(String::from)
            .collect();
        let t = parse_add_input(&words);
        assert_eq!(t.title, "Buy groceries");
        assert_eq!(t.tags, vec!["errands"]);
        assert_eq!(t.priority, Some(Priority::High));
        assert_eq!(t.deadline, Some("2026-06-01".to_string()));
    }

    #[test]
    fn parses_daily_recur_flag() {
        let recurrence = parse_recur_flag(Some("daily"))
            .expect("parse recur flag")
            .expect("recurrence json");
        let spec = RecurrenceSpec::from_json(&recurrence).expect("parse recurrence json");

        assert_eq!(spec.pattern, RecurrencePattern::Daily);
        assert_eq!(spec.mode, RecurrenceMode::Periodic);
    }

    #[test]
    fn parses_interval_recur_flag() {
        let recurrence = parse_recur_flag(Some("every-2w"))
            .expect("parse recur flag")
            .expect("recurrence json");
        let spec = RecurrenceSpec::from_json(&recurrence).expect("parse recurrence json");

        assert_eq!(spec.pattern, RecurrencePattern::EveryNWeeks(2));
    }
}
