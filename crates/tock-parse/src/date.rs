//! Natural language date parsing helpers.

use core::cmp::min;
use time::{Date, Duration, Month, Time, Weekday};

/// Parse a natural-language date expression relative to `now`.
#[must_use]
pub fn parse_date(input: &str, now: Date) -> Option<Date> {
    let normalized = input.trim();
    if normalized.is_empty() {
        return None;
    }

    let lowercase = normalized.to_ascii_lowercase();
    match lowercase.as_str() {
        "today" => return Some(now),
        "tomorrow" => return shift_days(now, 1),
        "yesterday" => return shift_days(now, -1),
        "eod" | "eow" => return end_of_week(now),
        "eom" => return end_of_month(now),
        _ => {}
    }

    if let Some(date) = parse_relative_expression(&lowercase, now) {
        return Some(date);
    }

    if let Some(date) = parse_weekday_expression(&lowercase, now) {
        return Some(date);
    }

    parse_iso_date(normalized)
}

/// Parse a natural-language date-and-optional-time expression relative to `now`.
///
/// Accepts a date phrase understood by [`parse_date`] optionally followed by a
/// time-of-day, in any of these shapes:
/// - `2026-06-01` (date only)
/// - `2026-06-01T14:00` (ISO date+time)
/// - `2026-06-01 14:00`, `tomorrow 9am`, `next friday 15:30` (date phrase + time)
///
/// Returns `(date, Some(time))` when a time component is present, or
/// `(date, None)` for an all-day date. Returns `None` if the date cannot be
/// resolved.
#[must_use]
pub fn parse_datetime(input: &str, now: Date) -> Option<(Date, Option<Time>)> {
    let normalized = input.trim();
    if normalized.is_empty() {
        return None;
    }

    // ISO `T` separator: `<date>T<time>`.
    if let Some((date_part, time_part)) = normalized.split_once(['T', 't'])
        && let (Some(date), Some(time)) = (
            parse_date(date_part.trim(), now),
            parse_time(time_part.trim()),
        )
    {
        return Some((date, Some(time)));
    }

    // Trailing whitespace-separated time token: `<date phrase> <time>`.
    if let Some((phrase, last)) = normalized.rsplit_once(char::is_whitespace)
        && let Some(time) = parse_time(last.trim())
        && let Some(date) = parse_date(phrase.trim(), now)
    {
        return Some((date, Some(time)));
    }

    // No time component — treat the whole input as an all-day date.
    parse_date(normalized, now).map(|date| (date, None))
}

/// Format a resolved schedule slot into its stored string representation:
/// `YYYY-MM-DD` for an all-day slot or `YYYY-MM-DDTHH:MM` for a timed slot.
#[must_use]
pub fn format_scheduled(date: Date, time: Option<Time>) -> String {
    let day = format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    );
    match time {
        Some(time) => format!("{day}T{:02}:{:02}", time.hour(), time.minute()),
        None => day,
    }
}

/// Parse a time-of-day token such as `14:00`, `9:30`, `9am`, `12pm`, or `9`.
fn parse_time(input: &str) -> Option<Time> {
    let trimmed = input.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }

    let meridiem = if trimmed.ends_with("am") {
        Some(false)
    } else if trimmed.ends_with("pm") {
        Some(true)
    } else {
        None
    };
    let body = trimmed
        .strip_suffix("am")
        .or_else(|| trimmed.strip_suffix("pm"))
        .unwrap_or(trimmed.as_str())
        .trim();

    let (hour_str, minute_str) = match body.split_once(':') {
        Some((h, m)) => (h, m),
        None => (body, "0"),
    };

    let mut hour: u8 = hour_str.parse().ok()?;
    let minute: u8 = minute_str.parse().ok()?;
    if minute > 59 {
        return None;
    }

    match meridiem {
        Some(is_pm) => {
            // 12-hour clock: 12am -> 00, 12pm -> 12, otherwise add 12 for PM.
            if hour == 0 || hour > 12 {
                return None;
            }
            if hour == 12 {
                hour = 0;
            }
            if is_pm {
                hour += 12;
            }
        }
        None => {
            if hour > 23 {
                return None;
            }
        }
    }

    Time::from_hms(hour, minute, 0).ok()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RelativeUnit {
    Day,
    Week,
    Month,
}

fn parse_relative_expression(input: &str, now: Date) -> Option<Date> {
    let tokens: Vec<_> = input.split_whitespace().collect();
    match tokens.as_slice() {
        ["in", amount, unit] => {
            apply_relative_offset(now, parse_amount(amount)?, parse_unit(unit)?, true)
        }
        [amount, unit, "ago"] => {
            apply_relative_offset(now, parse_amount(amount)?, parse_unit(unit)?, false)
        }
        _ => None,
    }
}

fn parse_amount(input: &str) -> Option<i64> {
    input.parse::<i64>().ok()
}

fn parse_unit(input: &str) -> Option<RelativeUnit> {
    match input {
        "day" | "days" => Some(RelativeUnit::Day),
        "week" | "weeks" => Some(RelativeUnit::Week),
        "month" | "months" => Some(RelativeUnit::Month),
        _ => None,
    }
}

fn apply_relative_offset(
    now: Date,
    amount: i64,
    unit: RelativeUnit,
    forward: bool,
) -> Option<Date> {
    match unit {
        RelativeUnit::Day => {
            let signed_amount = apply_direction(amount, forward)?;
            shift_days(now, signed_amount)
        }
        RelativeUnit::Week => {
            let days = amount.checked_mul(7)?;
            let signed_amount = apply_direction(days, forward)?;
            shift_days(now, signed_amount)
        }
        RelativeUnit::Month => {
            let months = i32::try_from(amount).ok()?;
            let signed_months = if forward {
                months
            } else {
                months.checked_neg()?
            };
            add_months(now, signed_months)
        }
    }
}

const fn apply_direction(amount: i64, forward: bool) -> Option<i64> {
    if forward {
        Some(amount)
    } else {
        amount.checked_neg()
    }
}

fn parse_weekday_expression(input: &str, now: Date) -> Option<Date> {
    let tokens: Vec<_> = input.split_whitespace().collect();
    match tokens.as_slice() {
        [weekday] | ["next", weekday] => next_weekday(now, parse_weekday(weekday)?),
        ["this", weekday] => this_weekday(now, parse_weekday(weekday)?),
        _ => None,
    }
}

fn parse_weekday(input: &str) -> Option<Weekday> {
    match input {
        "monday" => Some(Weekday::Monday),
        "tuesday" => Some(Weekday::Tuesday),
        "wednesday" => Some(Weekday::Wednesday),
        "thursday" => Some(Weekday::Thursday),
        "friday" => Some(Weekday::Friday),
        "saturday" => Some(Weekday::Saturday),
        "sunday" => Some(Weekday::Sunday),
        _ => None,
    }
}

fn next_weekday(now: Date, target: Weekday) -> Option<Date> {
    let current = i64::from(now.weekday().number_days_from_monday());
    let target = i64::from(target.number_days_from_monday());
    let mut days_ahead = (target - current).rem_euclid(7);
    if days_ahead == 0 {
        days_ahead = 7;
    }

    shift_days(now, days_ahead)
}

fn this_weekday(now: Date, target: Weekday) -> Option<Date> {
    let current = i64::from(now.weekday().number_days_from_monday());
    let target = i64::from(target.number_days_from_monday());
    shift_days(now, target - current)
}

fn end_of_week(now: Date) -> Option<Date> {
    let days_until_sunday = 6_i64 - i64::from(now.weekday().number_days_from_monday());
    shift_days(now, days_until_sunday)
}

fn end_of_month(now: Date) -> Option<Date> {
    Date::from_calendar_date(now.year(), now.month(), now.month().length(now.year())).ok()
}

fn shift_days(date: Date, amount: i64) -> Option<Date> {
    if amount >= 0 {
        date.checked_add(Duration::days(amount))
    } else {
        date.checked_sub(Duration::days(amount.checked_neg()?))
    }
}

fn add_months(date: Date, months: i32) -> Option<Date> {
    let current_month_index = i32::from(u8::from(date.month())) - 1;
    let total_months = date
        .year()
        .checked_mul(12)?
        .checked_add(current_month_index)?
        .checked_add(months)?;

    let target_year = total_months.div_euclid(12);
    let target_month_index = total_months.rem_euclid(12);
    let target_month_number = u8::try_from(target_month_index + 1).ok()?;
    let target_month = Month::try_from(target_month_number).ok()?;
    let target_day = min(date.day(), target_month.length(target_year));

    Date::from_calendar_date(target_year, target_month, target_day).ok()
}

fn parse_iso_date(input: &str) -> Option<Date> {
    let mut segments = input.split('-');
    let year = segments.next()?.parse::<i32>().ok()?;
    let month_number = segments.next()?.parse::<u8>().ok()?;
    let day = segments.next()?.parse::<u8>().ok()?;
    if segments.next().is_some() {
        return None;
    }

    let month = Month::try_from(month_number).ok()?;
    Date::from_calendar_date(year, month, day).ok()
}

#[cfg(test)]
mod tests {
    use super::{format_scheduled, parse_date, parse_datetime};
    use time::macros::{date, time};

    #[test]
    fn parses_today() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("today", now), Some(now));
    }

    #[test]
    fn parses_tomorrow() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("tomorrow", now), Some(date!(2026 - 06 - 18)));
    }

    #[test]
    fn parses_yesterday() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("yesterday", now), Some(date!(2026 - 06 - 16)));
    }

    #[test]
    fn parses_in_days() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("in 3 days", now), Some(date!(2026 - 06 - 20)));
    }

    #[test]
    fn parses_in_weeks() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("in 2 weeks", now), Some(date!(2026 - 07 - 01)));
    }

    #[test]
    fn parses_in_months_with_end_of_month_clamp() {
        let now = date!(2026 - 01 - 31);
        assert_eq!(parse_date("in 1 month", now), Some(date!(2026 - 02 - 28)));
    }

    #[test]
    fn parses_days_ago() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("3 days ago", now), Some(date!(2026 - 06 - 14)));
    }

    #[test]
    fn parses_weeks_ago() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("2 weeks ago", now), Some(date!(2026 - 06 - 03)));
    }

    #[test]
    fn parses_months_ago_with_end_of_month_clamp() {
        let now = date!(2026 - 03 - 31);
        assert_eq!(parse_date("1 month ago", now), Some(date!(2026 - 02 - 28)));
    }

    #[test]
    fn parses_end_of_week() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("eow", now), Some(date!(2026 - 06 - 21)));
    }

    #[test]
    fn parses_end_of_day_alias_as_end_of_week() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("eod", now), Some(date!(2026 - 06 - 21)));
    }

    #[test]
    fn parses_end_of_month() {
        let now = date!(2026 - 02 - 15);
        assert_eq!(parse_date("eom", now), Some(date!(2026 - 02 - 28)));
    }

    #[test]
    fn parses_next_weekday_from_same_week() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("friday", now), Some(date!(2026 - 06 - 19)));
    }

    #[test]
    fn bare_weekday_never_returns_today() {
        let now = date!(2026 - 06 - 15);
        assert_eq!(parse_date("monday", now), Some(date!(2026 - 06 - 22)));
    }

    #[test]
    fn parses_next_weekday_keyword() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("next tuesday", now), Some(date!(2026 - 06 - 23)));
    }

    #[test]
    fn parses_this_weekday_in_past() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("this monday", now), Some(date!(2026 - 06 - 15)));
    }

    #[test]
    fn parses_this_weekday_in_future() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("this friday", now), Some(date!(2026 - 06 - 19)));
    }

    #[test]
    fn parses_absolute_iso_date() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("2026-06-15", now), Some(date!(2026 - 06 - 15)));
    }

    #[test]
    fn rejects_invalid_input() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("blursday", now), None);
    }

    #[test]
    fn matches_case_insensitively() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_date("IN 1 DAY", now), Some(date!(2026 - 06 - 18)));
    }

    #[test]
    fn parse_datetime_date_only() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(
            parse_datetime("2026-06-20", now),
            Some((date!(2026 - 06 - 20), None))
        );
    }

    #[test]
    fn parse_datetime_iso_t_separator() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(
            parse_datetime("2026-06-20T14:30", now),
            Some((date!(2026 - 06 - 20), Some(time!(14:30))))
        );
    }

    #[test]
    fn parse_datetime_space_separated_time() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(
            parse_datetime("2026-06-20 09:15", now),
            Some((date!(2026 - 06 - 20), Some(time!(9:15))))
        );
    }

    #[test]
    fn parse_datetime_natural_phrase_with_time() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(
            parse_datetime("tomorrow 9am", now),
            Some((date!(2026 - 06 - 18), Some(time!(9:00))))
        );
    }

    #[test]
    fn parse_datetime_weekday_phrase_with_pm_time() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(
            parse_datetime("friday 3:30pm", now),
            Some((date!(2026 - 06 - 19), Some(time!(15:30))))
        );
    }

    #[test]
    fn parse_datetime_noon_and_midnight_meridiem() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(
            parse_datetime("2026-06-20 12pm", now),
            Some((date!(2026 - 06 - 20), Some(time!(12:00))))
        );
        assert_eq!(
            parse_datetime("2026-06-20 12am", now),
            Some((date!(2026 - 06 - 20), Some(time!(0:00))))
        );
    }

    #[test]
    fn parse_datetime_rejects_time_only() {
        let now = date!(2026 - 06 - 17);
        assert_eq!(parse_datetime("9am", now), None);
    }

    #[test]
    fn parse_datetime_rejects_invalid_time() {
        let now = date!(2026 - 06 - 17);
        // 25:00 is not a valid time; with no valid trailing time the whole
        // string fails to parse as a date either.
        assert_eq!(parse_datetime("2026-06-20 25:00", now), None);
    }

    #[test]
    fn format_scheduled_roundtrips() {
        assert_eq!(
            format_scheduled(date!(2026 - 06 - 20), None),
            "2026-06-20".to_string()
        );
        assert_eq!(
            format_scheduled(date!(2026 - 06 - 20), Some(time!(14:05))),
            "2026-06-20T14:05".to_string()
        );
    }
}
