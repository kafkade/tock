//! Built-in views — smart filters for common task perspectives.
//!
//! Per architecture §2.1.3, views are seeded saved queries. This
//! module defines the six core views and their default filter
//! definitions.

use tock_parse::filter::Filter;

/// A named built-in view.
#[derive(Clone, Debug)]
pub struct View {
    /// View name (e.g. "inbox", "today").
    pub name: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// Filter to apply.
    pub filter: Filter,
}

/// Build the Inbox view filter: `status:inbox`.
#[must_use]
pub fn inbox() -> View {
    View {
        name: "inbox",
        description: "Unprocessed tasks",
        filter: Filter::Status("inbox".to_string()),
    }
}

/// Build the Today view filter: tasks that are pending/started and
/// have a deadline <= today or `start_date` <= today.
#[must_use]
pub fn today(today_str: &str) -> View {
    View {
        name: "today",
        description: "Due or starting today",
        filter: Filter::And(vec![
            Filter::Or(vec![
                Filter::Status("pending".to_string()),
                Filter::Status("started".to_string()),
            ]),
            Filter::Today {
                today: today_str.to_string(),
            },
        ]),
    }
}

/// Build the Upcoming view filter: tasks with a future deadline or
/// start date.
#[must_use]
pub fn upcoming(today_str: &str) -> View {
    View {
        name: "upcoming",
        description: "Coming up in the next 7+ days",
        filter: Filter::And(vec![
            Filter::Or(vec![
                Filter::Status("pending".to_string()),
                Filter::Status("started".to_string()),
            ]),
            Filter::Not(Box::new(Filter::Today {
                today: today_str.to_string(),
            })),
            Filter::HasDeadline,
        ]),
    }
}

/// Build the Anytime view filter: pending tasks with no date.
#[must_use]
pub fn anytime() -> View {
    View {
        name: "anytime",
        description: "Ready tasks with no date",
        filter: Filter::And(vec![
            Filter::Or(vec![
                Filter::Status("pending".to_string()),
                Filter::Status("started".to_string()),
            ]),
            Filter::Not(Box::new(Filter::HasDeadline)),
        ]),
    }
}

/// Build the Someday view filter: `status:someday`.
#[must_use]
pub fn someday() -> View {
    View {
        name: "someday",
        description: "Deferred indefinitely",
        filter: Filter::Status("someday".to_string()),
    }
}

/// Build the Logbook view filter: completed or cancelled tasks.
#[must_use]
pub fn logbook() -> View {
    View {
        name: "logbook",
        description: "Completed and cancelled tasks",
        filter: Filter::Or(vec![
            Filter::Status("done".to_string()),
            Filter::Status("cancelled".to_string()),
        ]),
    }
}

/// All built-in views in display order.
#[must_use]
pub fn all_views(today_str: &str) -> Vec<View> {
    vec![
        inbox(),
        today(today_str),
        upcoming(today_str),
        anytime(),
        someday(),
        logbook(),
    ]
}
