//! Bidirectional field mapping between tock domain types and iCalendar
//! properties per architecture §9.5.
//!
//! Converts `Task` → `VTODO`, `TimeBlock` → `VEVENT`, and back.

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use tock_core::domain::task::{NewTask, Priority, Task, TaskStatus};
use tock_core::domain::time_block::TimeBlock;

use crate::Error;
use crate::ical::{Component, Property, escape_text, unescape_text};

// ── Priority mapping (§9.5) ───────────────────────────────────────

/// Map tock Priority to iCalendar PRIORITY (1–9).
/// H → 1, M → 5, L → 9.
#[must_use]
pub const fn priority_to_ical(p: Priority) -> u8 {
    match p {
        Priority::High => 1,
        Priority::Medium => 5,
        Priority::Low => 9,
    }
}

/// Map iCalendar PRIORITY (1–9) to tock Priority.
/// 1–4 → H, 5 → M, 6–9 → L, 0 → None.
#[must_use]
pub const fn priority_from_ical(val: u8) -> Option<Priority> {
    match val {
        1..=4 => Some(Priority::High),
        5 => Some(Priority::Medium),
        6..=9 => Some(Priority::Low),
        _ => None,
    }
}

// ── Status mapping ────────────────────────────────────────────────

/// Map tock `TaskStatus` to iCalendar `STATUS` value for VTODO.
#[must_use]
pub const fn status_to_ical(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Inbox | TaskStatus::Pending | TaskStatus::Someday => "NEEDS-ACTION",
        TaskStatus::Started => "IN-PROCESS",
        TaskStatus::Done => "COMPLETED",
        TaskStatus::Cancelled => "CANCELLED",
    }
}

/// Map iCalendar `STATUS` to tock `TaskStatus`.
#[must_use]
pub fn status_from_ical(val: &str) -> TaskStatus {
    match val.to_uppercase().as_str() {
        "IN-PROCESS" => TaskStatus::Started,
        "COMPLETED" => TaskStatus::Done,
        "CANCELLED" => TaskStatus::Cancelled,
        _ => TaskStatus::Pending,
    }
}

// ── DateTime helpers ──────────────────────────────────────────────

/// Format an `OffsetDateTime` as iCalendar UTC datetime (`YYYYMMDDTHHmmssZ`).
#[must_use]
pub fn datetime_to_ical(dt: OffsetDateTime) -> String {
    let utc = dt.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        utc.year(),
        u8::from(utc.month()),
        utc.day(),
        utc.hour(),
        utc.minute(),
        utc.second()
    )
}

/// Format a date string (YYYY-MM-DD) as iCalendar DATE (YYYYMMDD).
#[must_use]
pub fn date_to_ical(date_str: &str) -> String {
    date_str.replace('-', "")
}

/// Parse iCalendar DATE (YYYYMMDD) back to YYYY-MM-DD.
#[must_use]
pub fn date_from_ical(val: &str) -> Option<String> {
    // Handle both YYYYMMDD and YYYYMMDDTHHmmssZ formats.
    let date_part = if val.len() >= 8 {
        &val[..8]
    } else {
        return None;
    };
    if date_part.len() == 8 && date_part.chars().all(|c| c.is_ascii_digit()) {
        Some(format!(
            "{}-{}-{}",
            &date_part[..4],
            &date_part[4..6],
            &date_part[6..8]
        ))
    } else {
        None
    }
}

/// Parse iCalendar UTC datetime (`YYYYMMDDTHHmmssZ`) to `OffsetDateTime`.
///
/// # Errors
/// Returns `Error::IcalParse` if the format is invalid.
pub fn datetime_from_ical(val: &str) -> Result<OffsetDateTime, Error> {
    // Try RFC 3339 first (some servers use it).
    if let Ok(dt) = OffsetDateTime::parse(val, &Rfc3339) {
        return Ok(dt);
    }
    // Try basic iCal format: YYYYMMDDTHHmmssZ
    if val.len() >= 15 && val.contains('T') {
        let clean = val.replace('Z', "+00:00");
        // Convert to RFC 3339: YYYY-MM-DDTHH:MM:SS+00:00
        let rfc = format!(
            "{}-{}-{}T{}:{}:{}{}",
            &val[..4],
            &val[4..6],
            &val[6..8],
            &val[9..11],
            &val[11..13],
            &val[13..15],
            if val.ends_with('Z') {
                "+00:00"
            } else if clean.len() > 15 {
                &clean[15..]
            } else {
                "+00:00"
            }
        );
        OffsetDateTime::parse(&rfc, &Rfc3339)
            .map_err(|e| Error::IcalParse(format!("datetime: {val}: {e}")))
    } else {
        Err(Error::IcalParse(format!(
            "unsupported datetime format: {val}"
        )))
    }
}

// ── Task ↔ VTODO ──────────────────────────────────────────────────

/// Convert a `Task` to a VTODO [`Component`].
///
/// The `uid` parameter is the `CalDAV` UID for this resource (may differ
/// from the task's internal UUID).
#[must_use]
pub fn task_to_vtodo(task: &Task, uid: &str) -> Component {
    let mut vtodo = Component::new("VTODO");

    vtodo.add_prop(Property::new("UID", uid));
    vtodo.add_prop(Property::new("SUMMARY", &escape_text(&task.title)));
    vtodo.add_prop(Property::new("STATUS", status_to_ical(task.status)));

    if let Some(ref notes) = task.notes {
        vtodo.add_prop(Property::new("DESCRIPTION", &escape_text(notes)));
    }
    if let Some(ref deadline) = task.deadline {
        vtodo.add_prop(Property::with_params(
            "DUE",
            vec![("VALUE".into(), "DATE".into())],
            &date_to_ical(deadline),
        ));
    }
    if let Some(ref start) = task.start_date {
        vtodo.add_prop(Property::with_params(
            "DTSTART",
            vec![("VALUE".into(), "DATE".into())],
            &date_to_ical(start),
        ));
    }
    if let Some(priority) = task.priority {
        vtodo.add_prop(Property::new(
            "PRIORITY",
            &priority_to_ical(priority).to_string(),
        ));
    }
    if !task.tags.is_empty() {
        vtodo.add_prop(Property::new("CATEGORIES", &task.tags.join(",")));
    }

    // Dependencies as RELATED-TO.
    for dep_id in &task.depends_on {
        vtodo.add_prop(Property::with_params(
            "RELATED-TO",
            vec![("RELTYPE".into(), "DEPENDS-ON".into())],
            &dep_id.to_string(),
        ));
    }

    // Recurrence.
    if let Some(ref rrule) = task.recurrence {
        vtodo.add_prop(Property::new("RRULE", rrule));
    }

    // Timestamps.
    vtodo.add_prop(Property::new("CREATED", &datetime_to_ical(task.created_at)));
    vtodo.add_prop(Property::new(
        "LAST-MODIFIED",
        &datetime_to_ical(task.modified_at),
    ));
    if let Some(done_at) = task.done_at {
        vtodo.add_prop(Property::new("COMPLETED", &datetime_to_ical(done_at)));
    }

    // tock-specific extensions.
    if task.evening {
        vtodo.add_prop(Property::new("X-APP-EVENING", "TRUE"));
    }
    if let Some(project_id) = task.project_id {
        vtodo.add_prop(Property::new("X-APP-PROJECT-ID", &project_id.to_string()));
    }
    vtodo.add_prop(Property::new("X-APP-TASK-ID", &task.id.to_string()));

    // UDAs as X-APP-UDA-* properties.
    for (key, value) in &task.udas.0 {
        vtodo.add_prop(Property::new(
            &format!("X-APP-UDA-{}", key.to_uppercase()),
            &value.to_string(),
        ));
    }

    vtodo
}

/// Build a `NewTask` from a parsed VTODO component.
///
/// # Errors
/// Returns `Error::Mapping` for invalid/missing required fields.
pub fn vtodo_to_new_task(vtodo: &Component) -> Result<NewTask, Error> {
    let title = vtodo
        .prop_value("SUMMARY")
        .map(unescape_text)
        .ok_or_else(|| Error::Mapping("VTODO missing SUMMARY".into()))?;

    let notes = vtodo.prop_value("DESCRIPTION").map(unescape_text);

    let status = vtodo.prop_value("STATUS").map(status_from_ical);

    let priority = vtodo
        .prop_value("PRIORITY")
        .and_then(|v| v.parse::<u8>().ok())
        .and_then(priority_from_ical);

    let deadline = vtodo.prop_value("DUE").and_then(date_from_ical);

    let start_date = vtodo.prop_value("DTSTART").and_then(date_from_ical);

    let tags: Vec<String> = vtodo
        .prop_value("CATEGORIES")
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let evening = vtodo
        .prop_value("X-APP-EVENING")
        .is_some_and(|v| v.eq_ignore_ascii_case("TRUE"));

    let recurrence = vtodo.prop_value("RRULE").map(String::from);

    // UDAs from X-APP-UDA-* properties.
    let mut udas = tock_core::domain::uda::UdaValues::default();
    for prop in vtodo.props_prefixed("X-APP-UDA-") {
        let key = prop
            .name
            .strip_prefix("X-APP-UDA-")
            .unwrap_or(&prop.name)
            .to_lowercase();
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&prop.value) {
            udas.set(&key, val);
        } else {
            udas.set(&key, serde_json::Value::String(prop.value.clone()));
        }
    }

    Ok(NewTask {
        title,
        notes,
        status,
        priority,
        deadline,
        start_date,
        evening,
        tags,
        recurrence,
        udas,
        ..NewTask::default()
    })
}

// ── TimeBlock ↔ VEVENT ────────────────────────────────────────────

/// Convert a `TimeBlock` to a VEVENT [`Component`].
#[must_use]
pub fn time_block_to_vevent(block: &TimeBlock, uid: &str) -> Component {
    let mut vevent = Component::new("VEVENT");

    vevent.add_prop(Property::new("UID", uid));
    vevent.add_prop(Property::new("SUMMARY", &escape_text(&block.title)));
    vevent.add_prop(Property::new("DTSTART", &datetime_to_ical(block.start_ts)));

    if let Some(end_ts) = block.end_ts {
        vevent.add_prop(Property::new("DTEND", &datetime_to_ical(end_ts)));
    }
    if let Some(ref notes) = block.notes {
        vevent.add_prop(Property::new("DESCRIPTION", &escape_text(notes)));
    }
    if block.billable {
        vevent.add_prop(Property::new("X-APP-BILLABLE", "TRUE"));
    }
    vevent.add_prop(Property::new(
        "X-APP-TASK-ID",
        &block.task_id.map_or_else(String::new, |id| id.to_string()),
    ));
    vevent.add_prop(Property::new("X-APP-BLOCK-ID", &block.id.to_string()));
    vevent.add_prop(Property::new("X-APP-SOURCE", block.source.as_str()));

    vevent
}

/// Parse VEVENT into time block fields. Returns `(title, start, end, task_id, notes, billable)`.
///
/// # Errors
/// Returns `Error::Mapping` for invalid fields.
pub fn vevent_to_time_block_fields(vevent: &Component) -> Result<TimeBlockFields, Error> {
    let title = vevent
        .prop_value("SUMMARY")
        .map(unescape_text)
        .ok_or_else(|| Error::Mapping("VEVENT missing SUMMARY".into()))?;

    let start = vevent
        .prop_value("DTSTART")
        .ok_or_else(|| Error::Mapping("VEVENT missing DTSTART".into()))
        .and_then(datetime_from_ical)?;

    let end = vevent
        .prop_value("DTEND")
        .map(datetime_from_ical)
        .transpose()?;

    let task_id = vevent
        .prop_value("X-APP-TASK-ID")
        .filter(|v| !v.is_empty())
        .and_then(|v| Uuid::parse_str(v).ok());

    let notes = vevent.prop_value("DESCRIPTION").map(unescape_text);

    let billable = vevent
        .prop_value("X-APP-BILLABLE")
        .is_some_and(|v| v.eq_ignore_ascii_case("TRUE"));

    Ok(TimeBlockFields {
        title,
        start,
        end,
        task_id,
        notes,
        billable,
    })
}

/// Parsed time block fields from a VEVENT.
#[derive(Clone, Debug)]
pub struct TimeBlockFields {
    /// Block title.
    pub title: String,
    /// Start timestamp.
    pub start: OffsetDateTime,
    /// End timestamp (None if still running).
    pub end: Option<OffsetDateTime>,
    /// Linked task ID.
    pub task_id: Option<Uuid>,
    /// Notes.
    pub notes: Option<String>,
    /// Billable flag.
    pub billable: bool,
}

// ── VCALENDAR wrapper ─────────────────────────────────────────────

/// Wrap a VTODO or VEVENT in a VCALENDAR envelope.
#[must_use]
pub fn wrap_vcalendar(inner: Component) -> Component {
    let mut cal = Component::new("VCALENDAR");
    cal.add_prop(Property::new("VERSION", "2.0"));
    cal.add_prop(Property::new("PRODID", "-//tock//caldav//EN"));
    cal.children.push(inner);
    cal
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use tock_core::domain::uda::UdaValues;

    fn make_test_task() -> Task {
        Task {
            id: Uuid::nil(),
            sid: 1,
            title: "Buy milk".into(),
            notes: Some("From the store\non Main St".into()),
            status: TaskStatus::Pending,
            area_id: None,
            project_id: None,
            heading_id: None,
            parent_id: None,
            start_date: Some("2026-06-01".into()),
            deadline: Some("2026-06-15".into()),
            recurrence: None,
            priority: Some(Priority::High),
            evening: true,
            udas: UdaValues::default(),
            tags: vec!["groceries".into(), "errands".into()],
            depends_on: vec![],
            checklist: vec![],
            urgency: 0.0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            modified_at: OffsetDateTime::UNIX_EPOCH,
            done_at: None,
            cancelled_at: None,
            deleted_at: None,
        }
    }

    #[test]
    fn priority_mapping_roundtrip() {
        assert_eq!(priority_to_ical(Priority::High), 1);
        assert_eq!(priority_to_ical(Priority::Medium), 5);
        assert_eq!(priority_to_ical(Priority::Low), 9);
        assert_eq!(priority_from_ical(1), Some(Priority::High));
        assert_eq!(priority_from_ical(3), Some(Priority::High));
        assert_eq!(priority_from_ical(5), Some(Priority::Medium));
        assert_eq!(priority_from_ical(7), Some(Priority::Low));
        assert_eq!(priority_from_ical(0), None);
    }

    #[test]
    fn status_mapping_roundtrip() {
        assert_eq!(status_to_ical(TaskStatus::Pending), "NEEDS-ACTION");
        assert_eq!(status_to_ical(TaskStatus::Started), "IN-PROCESS");
        assert_eq!(status_to_ical(TaskStatus::Done), "COMPLETED");
        assert_eq!(status_to_ical(TaskStatus::Cancelled), "CANCELLED");
        assert_eq!(status_from_ical("NEEDS-ACTION"), TaskStatus::Pending);
        assert_eq!(status_from_ical("IN-PROCESS"), TaskStatus::Started);
        assert_eq!(status_from_ical("COMPLETED"), TaskStatus::Done);
        assert_eq!(status_from_ical("CANCELLED"), TaskStatus::Cancelled);
    }

    #[test]
    fn task_to_vtodo_and_back() {
        let task = make_test_task();
        let vtodo = task_to_vtodo(&task, "uid-123");
        assert_eq!(vtodo.prop_value("UID"), Some("uid-123"));
        assert_eq!(vtodo.prop_value("STATUS"), Some("NEEDS-ACTION"));
        assert_eq!(vtodo.prop_value("PRIORITY"), Some("1"));
        assert_eq!(vtodo.prop_value("X-APP-EVENING"), Some("TRUE"));

        let new_task = vtodo_to_new_task(&vtodo).expect("mapping");
        assert_eq!(new_task.title, task.title);
        assert_eq!(new_task.priority, task.priority);
        assert_eq!(new_task.evening, task.evening);
        assert_eq!(new_task.tags, task.tags);
        assert_eq!(new_task.deadline, task.deadline);
        assert_eq!(new_task.start_date, task.start_date);
    }

    #[test]
    fn date_conversion_roundtrip() {
        assert_eq!(date_to_ical("2026-06-15"), "20260615");
        assert_eq!(date_from_ical("20260615"), Some("2026-06-15".into()));
        assert_eq!(
            date_from_ical("20260615T120000Z"),
            Some("2026-06-15".into())
        );
    }

    #[test]
    fn datetime_conversion_roundtrip() {
        let dt = OffsetDateTime::UNIX_EPOCH;
        let ical = datetime_to_ical(dt);
        assert_eq!(ical, "19700101T000000Z");
        let parsed = datetime_from_ical(&ical).expect("parse");
        assert_eq!(parsed, dt);
    }

    #[test]
    fn vtodo_missing_summary_errors() {
        let vtodo = Component::new("VTODO");
        assert!(vtodo_to_new_task(&vtodo).is_err());
    }

    #[test]
    fn vcalendar_wrapper() {
        let vtodo = Component::new("VTODO");
        let cal = wrap_vcalendar(vtodo);
        assert_eq!(cal.kind, "VCALENDAR");
        assert_eq!(cal.prop_value("VERSION"), Some("2.0"));
        assert_eq!(cal.children.len(), 1);
        assert_eq!(cal.children[0].kind, "VTODO");
    }
}
