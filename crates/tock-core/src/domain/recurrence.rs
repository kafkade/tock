//! Recurrence engine for tasks.

use serde_json::{Value, json};
use time::{Date, Duration, Month, macros::format_description};

/// Recurrence mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecurrenceMode {
    /// Next instance appears on the due date of the current one.
    Periodic,
    /// Next instance appears relative to when the current one is completed.
    Chained,
}

/// Recurrence pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecurrencePattern {
    /// Repeat every day.
    Daily,
    /// Repeat every week.
    Weekly,
    /// Repeat every month.
    Monthly,
    /// Repeat every year.
    Yearly,
    /// Repeat every `n` days.
    EveryNDays(u32),
    /// Repeat every `n` weeks.
    EveryNWeeks(u32),
}

/// Full recurrence spec stored as JSON on the task.
#[derive(Clone, Debug)]
pub struct RecurrenceSpec {
    /// Recurrence cadence.
    pub pattern: RecurrencePattern,
    /// How the next instance is anchored.
    pub mode: RecurrenceMode,
}

impl RecurrenceSpec {
    /// Parse a recurrence spec from stored JSON.
    #[must_use]
    pub fn from_json(s: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(s).ok()?;
        let mode = match value.get("mode")?.as_str()? {
            "periodic" => RecurrenceMode::Periodic,
            "chained" => RecurrenceMode::Chained,
            _ => return None,
        };
        let pattern = match value.get("pattern")?.as_str()? {
            "daily" => RecurrencePattern::Daily,
            "weekly" => RecurrencePattern::Weekly,
            "monthly" => RecurrencePattern::Monthly,
            "yearly" => RecurrencePattern::Yearly,
            "every_n_days" => RecurrencePattern::EveryNDays(parse_positive_interval(&value)?),
            "every_n_weeks" => RecurrencePattern::EveryNWeeks(parse_positive_interval(&value)?),
            _ => return None,
        };
        Some(Self { pattern, mode })
    }

    /// Serialize the recurrence spec to stored JSON.
    #[must_use]
    pub fn to_json(&self) -> String {
        let value = match self.pattern {
            RecurrencePattern::Daily => {
                json!({ "pattern": "daily", "mode": mode_name(&self.mode) })
            }
            RecurrencePattern::Weekly => {
                json!({ "pattern": "weekly", "mode": mode_name(&self.mode) })
            }
            RecurrencePattern::Monthly => {
                json!({ "pattern": "monthly", "mode": mode_name(&self.mode) })
            }
            RecurrencePattern::Yearly => {
                json!({ "pattern": "yearly", "mode": mode_name(&self.mode) })
            }
            RecurrencePattern::EveryNDays(days) => {
                json!({ "pattern": "every_n_days", "n": days, "mode": mode_name(&self.mode) })
            }
            RecurrencePattern::EveryNWeeks(weeks) => {
                json!({ "pattern": "every_n_weeks", "n": weeks, "mode": mode_name(&self.mode) })
            }
        };
        value.to_string()
    }

    /// Compute the next due date given the current due date or completion date.
    #[must_use]
    pub fn next_date(&self, from: &str, mode_anchor: &str) -> Option<String> {
        let anchor = match self.mode {
            RecurrenceMode::Periodic => from,
            RecurrenceMode::Chained => mode_anchor,
        };
        let date = parse_iso_date(anchor)?;
        let next = match self.pattern {
            RecurrencePattern::Daily => date.checked_add(Duration::days(1))?,
            RecurrencePattern::Weekly => date.checked_add(Duration::days(7))?,
            RecurrencePattern::Monthly => add_months(date, 1)?,
            RecurrencePattern::Yearly => add_years(date, 1)?,
            RecurrencePattern::EveryNDays(days) => {
                date.checked_add(Duration::days(i64::from(days)))?
            }
            RecurrencePattern::EveryNWeeks(weeks) => {
                date.checked_add(Duration::days(i64::from(weeks) * 7))?
            }
        };
        Some(format_iso_date(next))
    }
}

fn parse_positive_interval(value: &Value) -> Option<u32> {
    let interval = u32::try_from(value.get("n")?.as_u64()?).ok()?;
    if interval == 0 { None } else { Some(interval) }
}

const fn mode_name(mode: &RecurrenceMode) -> &'static str {
    match mode {
        RecurrenceMode::Periodic => "periodic",
        RecurrenceMode::Chained => "chained",
    }
}

fn parse_iso_date(raw: &str) -> Option<Date> {
    Date::parse(raw, &format_description!("[year]-[month]-[day]")).ok()
}

fn format_iso_date(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn add_months(date: Date, months: i32) -> Option<Date> {
    let start_month = i32::from(u8::from(date.month())) - 1;
    let total_months = start_month.checked_add(months)?;
    let year = date.year().checked_add(total_months.div_euclid(12))?;
    let month_number = total_months.rem_euclid(12) + 1;
    let month = Month::try_from(u8::try_from(month_number).ok()?).ok()?;
    let day = date.day().min(days_in_month(year, month));
    Date::from_calendar_date(year, month, day).ok()
}

fn add_years(date: Date, years: i32) -> Option<Date> {
    let year = date.year().checked_add(years)?;
    let month = date.month();
    let day = date.day().min(days_in_month(year, month));
    Date::from_calendar_date(year, month, day).ok()
}

fn days_in_month(year: i32, month: Month) -> u8 {
    for day in (28_u8..=31).rev() {
        if Date::from_calendar_date(year, month, day).is_ok() {
            return day;
        }
    }
    28
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{RecurrenceMode, RecurrencePattern, RecurrenceSpec};

    #[test]
    fn roundtrips_daily_json() {
        let spec = RecurrenceSpec {
            pattern: RecurrencePattern::Daily,
            mode: RecurrenceMode::Periodic,
        };

        let json = spec.to_json();
        let parsed = RecurrenceSpec::from_json(&json).expect("parse recurrence spec");
        assert_eq!(parsed.pattern, RecurrencePattern::Daily);
        assert_eq!(parsed.mode, RecurrenceMode::Periodic);
    }

    #[test]
    fn parses_interval_recurrence_json() {
        let spec =
            RecurrenceSpec::from_json(r#"{"pattern":"every_n_weeks","n":2,"mode":"chained"}"#)
                .expect("parse interval recurrence spec");

        assert_eq!(spec.pattern, RecurrencePattern::EveryNWeeks(2));
        assert_eq!(spec.mode, RecurrenceMode::Chained);
    }

    #[test]
    fn periodic_recurrence_uses_due_date_anchor() {
        let spec = RecurrenceSpec {
            pattern: RecurrencePattern::Weekly,
            mode: RecurrenceMode::Periodic,
        };

        assert_eq!(
            spec.next_date("2026-01-10", "2026-01-12"),
            Some(String::from("2026-01-17"))
        );
    }

    #[test]
    fn chained_recurrence_uses_completion_anchor() {
        let spec = RecurrenceSpec {
            pattern: RecurrencePattern::EveryNDays(3),
            mode: RecurrenceMode::Chained,
        };

        assert_eq!(
            spec.next_date("2026-01-10", "2026-01-12"),
            Some(String::from("2026-01-15"))
        );
    }

    #[test]
    fn monthly_recurrence_clamps_to_short_months() {
        let spec = RecurrenceSpec {
            pattern: RecurrencePattern::Monthly,
            mode: RecurrenceMode::Periodic,
        };

        assert_eq!(
            spec.next_date("2025-01-31", "2025-01-31"),
            Some(String::from("2025-02-28"))
        );
    }

    #[test]
    fn rejects_zero_intervals() {
        assert!(
            RecurrenceSpec::from_json(r#"{"pattern":"every_n_days","n":0,"mode":"periodic"}"#,)
                .is_none()
        );
    }
}
