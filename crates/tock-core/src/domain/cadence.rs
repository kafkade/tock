//! Cadence parsing and due-date calculation for habits.

use serde_json::Value;
use time::Weekday;

/// Parsed cadence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParsedCadence {
    /// Due every day.
    Daily,
    /// Due a target number of times per week.
    WeeklyTarget {
        /// Number of completions expected each week.
        times_per_week: u8,
    },
    /// Due on specific weekdays.
    SpecificDays {
        /// Days on which the habit is due.
        days: Vec<Weekday>,
    },
    /// Due every `n` days.
    EveryNDays {
        /// Interval between due dates.
        n: u8,
    },
}

impl ParsedCadence {
    /// Parse from the JSON string stored in the database.
    #[must_use]
    pub fn from_json(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.eq_ignore_ascii_case("daily") {
            return Some(Self::Daily);
        }

        let value = serde_json::from_str::<Value>(trimmed).ok()?;
        match value {
            Value::String(text) if text.eq_ignore_ascii_case("daily") => Some(Self::Daily),
            Value::Object(map) => {
                if let Some(times_per_week) = map.get("weekly_target").and_then(Value::as_u64) {
                    let times_per_week = u8::try_from(times_per_week).ok()?;
                    return (times_per_week > 0).then_some(Self::WeeklyTarget { times_per_week });
                }

                if let Some(days) = map.get("specific_days").and_then(Value::as_array) {
                    let parsed_days: Vec<_> = days
                        .iter()
                        .filter_map(Value::as_str)
                        .filter_map(parse_weekday)
                        .collect();
                    return (!parsed_days.is_empty())
                        .then_some(Self::SpecificDays { days: parsed_days });
                }

                if let Some(n) = map.get("every_n_days").and_then(Value::as_u64) {
                    let n = u8::try_from(n).ok()?;
                    return (n > 0).then_some(Self::EveryNDays { n });
                }

                None
            }
            _ => None,
        }
    }

    /// Serialize to JSON for storage.
    #[must_use]
    pub fn to_json(&self) -> String {
        match self {
            Self::Daily => String::from("\"daily\""),
            Self::WeeklyTarget { times_per_week } => {
                serde_json::json!({ "weekly_target": times_per_week }).to_string()
            }
            Self::SpecificDays { days } => serde_json::json!({
                "specific_days": days.iter().copied().map(weekday_storage_name).collect::<Vec<_>>()
            })
            .to_string(),
            Self::EveryNDays { n } => serde_json::json!({ "every_n_days": n }).to_string(),
        }
    }

    /// Human-readable display.
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Daily => String::from("daily"),
            Self::WeeklyTarget { times_per_week } => format!("{times_per_week}×/week"),
            Self::SpecificDays { days } => days
                .iter()
                .map(|day| weekday_display_name(*day))
                .collect::<Vec<_>>()
                .join("/"),
            Self::EveryNDays { n } => {
                let suffix = if *n == 1 { "day" } else { "days" };
                format!("every {n} {suffix}")
            }
        }
    }

    /// Whether this cadence expects completion on the given date.
    #[must_use]
    pub fn is_due_on(&self, date: time::Date) -> bool {
        match self {
            Self::Daily | Self::WeeklyTarget { .. } | Self::EveryNDays { .. } => true,
            Self::SpecificDays { days } => days.contains(&date.weekday()),
        }
    }
}

fn parse_weekday(day: &str) -> Option<Weekday> {
    match day.to_ascii_lowercase().as_str() {
        "mon" | "monday" => Some(Weekday::Monday),
        "tue" | "tues" | "tuesday" => Some(Weekday::Tuesday),
        "wed" | "wednesday" => Some(Weekday::Wednesday),
        "thu" | "thurs" | "thursday" => Some(Weekday::Thursday),
        "fri" | "friday" => Some(Weekday::Friday),
        "sat" | "saturday" => Some(Weekday::Saturday),
        "sun" | "sunday" => Some(Weekday::Sunday),
        _ => None,
    }
}

const fn weekday_storage_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Monday => "monday",
        Weekday::Tuesday => "tuesday",
        Weekday::Wednesday => "wednesday",
        Weekday::Thursday => "thursday",
        Weekday::Friday => "friday",
        Weekday::Saturday => "saturday",
        Weekday::Sunday => "sunday",
    }
}

const fn weekday_display_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Monday => "Mon",
        Weekday::Tuesday => "Tue",
        Weekday::Wednesday => "Wed",
        Weekday::Thursday => "Thu",
        Weekday::Friday => "Fri",
        Weekday::Saturday => "Sat",
        Weekday::Sunday => "Sun",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::ParsedCadence;
    use time::{Date, Month, Weekday};

    fn date(year: i32, month: Month, day: u8) -> Date {
        match Date::from_calendar_date(year, month, day) {
            Ok(value) => value,
            Err(error) => panic!("invalid test date: {error}"),
        }
    }

    #[test]
    fn parses_and_serializes_daily_cadence() {
        let cadence = ParsedCadence::from_json("\"daily\"");
        assert_eq!(cadence, Some(ParsedCadence::Daily));
        assert_eq!(ParsedCadence::Daily.to_json(), "\"daily\"");
        assert_eq!(ParsedCadence::Daily.display(), "daily");
    }

    #[test]
    fn parses_weekly_target_and_formats_display() {
        let cadence = ParsedCadence::from_json(r#"{"weekly_target":3}"#);
        assert_eq!(
            cadence,
            Some(ParsedCadence::WeeklyTarget { times_per_week: 3 })
        );
        assert_eq!(
            ParsedCadence::WeeklyTarget { times_per_week: 3 }.display(),
            "3×/week"
        );
    }

    #[test]
    fn parses_specific_days_and_checks_due_dates() {
        let cadence =
            ParsedCadence::from_json(r#"{"specific_days":["monday","wednesday","friday"]}"#);
        assert_eq!(
            cadence,
            Some(ParsedCadence::SpecificDays {
                days: vec![Weekday::Monday, Weekday::Wednesday, Weekday::Friday],
            })
        );

        let cadence = ParsedCadence::SpecificDays {
            days: vec![Weekday::Monday, Weekday::Wednesday, Weekday::Friday],
        };
        assert_eq!(cadence.display(), "Mon/Wed/Fri");
        assert!(cadence.is_due_on(date(2025, Month::May, 19)));
        assert!(!cadence.is_due_on(date(2025, Month::May, 20)));
    }

    #[test]
    fn parses_every_n_days_cadence() {
        let cadence = ParsedCadence::from_json(r#"{"every_n_days":3}"#);
        assert_eq!(cadence, Some(ParsedCadence::EveryNDays { n: 3 }));

        let cadence = ParsedCadence::EveryNDays { n: 3 };
        assert_eq!(cadence.display(), "every 3 days");
        assert!(cadence.is_due_on(date(2025, Month::May, 21)));
    }

    #[test]
    fn rejects_invalid_cadence_payloads() {
        assert_eq!(ParsedCadence::from_json(""), None);
        assert_eq!(ParsedCadence::from_json(r#"{"weekly_target":0}"#), None);
        assert_eq!(ParsedCadence::from_json(r#"{"specific_days":[]}"#), None);
        assert_eq!(ParsedCadence::from_json(r#"{"every_n_days":0}"#), None);
    }
}
