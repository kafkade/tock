//! Urgency scoring engine per architecture §2.1.4.
//!
//! All coefficients are configurable. The engine is pure computation —
//! it takes a task's fields and a config, returns a score.

/// Configurable urgency coefficients.
#[derive(Clone, Debug)]
pub struct UrgencyConfig {
    /// Weight for the deadline component.
    pub deadline_weight: f64,
    /// Weight for the start date component once the task is available.
    pub start_date_weight: f64,
    /// Weight for the priority component.
    pub priority_weight: f64,
    /// Weight for the task age component.
    pub age_weight: f64,
    /// Weight for the general tag-count component.
    pub tag_weight: f64,
    /// Weight for the project-assignment component.
    pub project_weight: f64,
    /// Weight for the `next` tag boost.
    pub next_weight: f64,
    /// Weight for the `blocked` tag penalty.
    pub blocked_weight: f64,
    /// Weight for the waiting penalty before a future start date.
    pub waiting_weight: f64,
}

impl Default for UrgencyConfig {
    fn default() -> Self {
        Self {
            deadline_weight: 1.0,
            start_date_weight: 0.8,
            priority_weight: 0.6,
            age_weight: 0.5,
            tag_weight: 0.4,
            project_weight: 0.4,
            next_weight: 0.7,
            blocked_weight: -5.0,
            waiting_weight: -2.0,
        }
    }
}

/// Inputs for urgency calculation — extracted from a task.
pub struct UrgencyInput<'a> {
    /// Optional priority in canonical single-letter form (`H`, `M`, `L`).
    pub priority: Option<char>,
    /// Optional deadline in `YYYY-MM-DD` form.
    pub deadline: Option<&'a str>,
    /// Optional deferred start date in `YYYY-MM-DD` form.
    pub start_date: Option<&'a str>,
    /// Task tag names.
    pub tags: &'a [String],
    /// Whether the task belongs to a project.
    pub has_project: bool,
    /// Age of the task in days.
    pub created_at_days_ago: f64,
    /// Today's date in `YYYY-MM-DD` form for comparisons.
    pub today: &'a str,
}

/// Calculate an urgency score from task inputs and coefficients.
#[must_use]
pub fn calculate(input: &UrgencyInput<'_>, config: &UrgencyConfig) -> f64 {
    explain(input, config)
        .into_iter()
        .map(|(_, _, _, contribution)| contribution)
        .sum()
}

/// Explain the urgency calculation.
///
/// Returns `(component, weight, factor, contribution)` tuples in evaluation order.
#[must_use]
pub fn explain(input: &UrgencyInput<'_>, config: &UrgencyConfig) -> Vec<(String, f64, f64, f64)> {
    let mut components = Vec::new();

    if let Some(deadline) = input.deadline {
        let factor = deadline_factor(input.today, deadline);
        components.push(component("deadline", config.deadline_weight, factor));
    }

    if let Some(start_date) = input.start_date {
        let days_until = days_between(input.today, start_date);
        if days_until <= 0 {
            components.push(component("start_date", config.start_date_weight, 1.0));
        } else {
            components.push(component("waiting", config.waiting_weight, 1.0));
        }
    }

    if let Some(priority) = input.priority {
        components.push(component(
            "priority",
            config.priority_weight,
            priority_factor(priority),
        ));
    }

    components.push(component(
        "age",
        config.age_weight,
        age_factor(input.created_at_days_ago),
    ));

    if input.has_project {
        components.push(component("project", config.project_weight, 1.0));
    }

    if has_tag(input.tags, "next") {
        components.push(component("next", config.next_weight, 1.0));
    }
    if has_tag(input.tags, "blocked") {
        components.push(component("blocked", config.blocked_weight, 1.0));
    }

    components.push(component(
        "tags",
        config.tag_weight,
        general_tag_factor(input.tags.len()),
    ));

    components
}

fn component(name: &str, weight: f64, factor: f64) -> (String, f64, f64, f64) {
    (name.to_string(), weight, factor, weight * factor)
}

fn deadline_factor(today: &str, deadline: &str) -> f64 {
    let days_until = days_between(today, deadline);
    if days_until < 0 {
        1.2
    } else if days_until == 0 {
        1.0
    } else if days_until <= 7 {
        0.8f64.mul_add(1.0 - f64::from(days_until) / 7.0, 0.2)
    } else if days_until <= 30 {
        0.2 * (1.0 - f64::from(days_until - 7) / 23.0)
    } else {
        0.0
    }
}

const fn priority_factor(priority: char) -> f64 {
    match priority {
        'H' | 'h' => 1.0,
        'M' | 'm' => 0.65,
        'L' | 'l' => 0.3,
        _ => 0.0,
    }
}

fn age_factor(created_at_days_ago: f64) -> f64 {
    (created_at_days_ago / 365.0).clamp(0.0, 1.0)
}

fn general_tag_factor(tag_count: usize) -> f64 {
    match tag_count.min(3) {
        0 => 0.0,
        1 => 0.33,
        2 => 0.66,
        _ => 0.99,
    }
}

fn has_tag(tags: &[String], needle: &str) -> bool {
    tags.iter().any(|tag| tag == needle)
}

/// Simple date difference in days (positive = future, negative = past).
fn days_between(today: &str, target: &str) -> i32 {
    match (parse_iso_date(today), parse_iso_date(target)) {
        (Some(today_days), Some(target_days)) => target_days - today_days,
        _ => 0,
    }
}

fn parse_iso_date(raw: &str) -> Option<i32> {
    let mut parts = raw.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<i32>().ok()?;
    let day = parts.next()?.parse::<i32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(year * 365 + month * 30 + day)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{UrgencyConfig, UrgencyInput, calculate, explain};

    fn input<'a>(
        priority: Option<char>,
        deadline: Option<&'a str>,
        start_date: Option<&'a str>,
        tags: &'a [String],
        created_at_days_ago: f64,
    ) -> UrgencyInput<'a> {
        UrgencyInput {
            priority,
            deadline,
            start_date,
            tags,
            has_project: false,
            created_at_days_ago,
            today: "2025-01-15",
        }
    }

    #[test]
    fn high_priority_scores_above_low_priority() {
        let tags = Vec::new();
        let low = calculate(
            &input(Some('L'), None, None, &tags, 0.0),
            &UrgencyConfig::default(),
        );
        let high = calculate(
            &input(Some('H'), None, None, &tags, 0.0),
            &UrgencyConfig::default(),
        );
        assert!(high > low);
    }

    #[test]
    fn overdue_task_scores_above_future_deadline() {
        let tags = Vec::new();
        let future = calculate(
            &input(None, Some("2025-02-20"), None, &tags, 0.0),
            &UrgencyConfig::default(),
        );
        let overdue = calculate(
            &input(None, Some("2025-01-01"), None, &tags, 0.0),
            &UrgencyConfig::default(),
        );
        assert!(overdue > future);
    }

    #[test]
    fn future_start_date_applies_waiting_penalty() {
        let tags = Vec::new();
        let available = calculate(
            &input(None, None, Some("2025-01-10"), &tags, 0.0),
            &UrgencyConfig::default(),
        );
        let waiting = calculate(
            &input(None, None, Some("2025-01-20"), &tags, 0.0),
            &UrgencyConfig::default(),
        );
        assert!(waiting < available);
    }

    #[test]
    fn age_increases_urgency_until_cap() {
        let tags = Vec::new();
        let younger = calculate(
            &input(None, None, None, &tags, 30.0),
            &UrgencyConfig::default(),
        );
        let older = calculate(
            &input(None, None, None, &tags, 180.0),
            &UrgencyConfig::default(),
        );
        let capped = calculate(
            &input(None, None, None, &tags, 730.0),
            &UrgencyConfig::default(),
        );
        let cap_edge = calculate(
            &input(None, None, None, &tags, 365.0),
            &UrgencyConfig::default(),
        );
        assert!(older > younger);
        assert!((capped - cap_edge).abs() < f64::EPSILON);
    }

    #[test]
    fn next_tag_boosts_urgency() {
        let baseline_tags = Vec::new();
        let next_tags = vec![String::from("next")];
        let baseline = calculate(
            &input(None, None, None, &baseline_tags, 0.0),
            &UrgencyConfig::default(),
        );
        let next = calculate(
            &input(None, None, None, &next_tags, 0.0),
            &UrgencyConfig::default(),
        );
        assert!(next > baseline);
    }

    #[test]
    fn blocked_tag_tanks_urgency() {
        let baseline_tags = Vec::new();
        let blocked_tags = vec![String::from("blocked")];
        let baseline = calculate(
            &input(None, None, None, &baseline_tags, 0.0),
            &UrgencyConfig::default(),
        );
        let blocked = calculate(
            &input(None, None, None, &blocked_tags, 0.0),
            &UrgencyConfig::default(),
        );
        assert!(blocked < baseline);
    }

    #[test]
    fn explain_returns_breakdown() {
        let tags = vec![String::from("next")];
        let input = input(Some('H'), Some("2025-01-15"), None, &tags, 10.0);
        let config = UrgencyConfig::default();
        let breakdown = explain(&input, &config);
        let explained_total: f64 = breakdown
            .iter()
            .map(|(_, _, _, contribution)| contribution)
            .sum();
        assert!(!breakdown.is_empty());
        assert!((explained_total - calculate(&input, &config)).abs() < f64::EPSILON);
    }
}
