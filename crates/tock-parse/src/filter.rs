//! Filter parsing and matching for task-like entities.

/// Trait that any filterable entity must implement.
pub trait Filterable {
    /// Return the entity status.
    fn status(&self) -> &str;

    /// Return the entity tags.
    fn tags(&self) -> &[String];

    /// Return the entity priority.
    fn priority(&self) -> Option<char>;

    /// Return the project name.
    fn project_name(&self) -> Option<&str>;

    /// Return the deadline as an ISO date string.
    fn deadline(&self) -> Option<&str>;

    /// Return the start date as an ISO date string.
    fn start_date(&self) -> Option<&str>;

    /// Return whether the entity is scheduled for the evening.
    fn is_evening(&self) -> bool;

    /// Return whether the entity is deleted.
    fn is_deleted(&self) -> bool;

    /// Return a user-defined attribute value as display text.
    fn uda_value(&self, key: &str) -> Option<String>;
}

/// A parsed filter expression.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Filter {
    /// Match an exact status.
    Status(String),
    /// Match an exact tag.
    Tag(String),
    /// Match an exact priority.
    Priority(char),
    /// Match a project name.
    Project(String),
    /// Match entities that have a deadline.
    HasDeadline,
    /// Match entities with a deadline before `today`.
    Overdue {
        /// The current day in ISO date format.
        today: String,
    },
    /// Match entities due or starting on or before `today`.
    Today {
        /// The current day in ISO date format.
        today: String,
    },
    /// Match evening entities.
    Evening,
    /// Match deleted entities.
    Deleted,
    /// Match an exact UDA value.
    Uda {
        /// UDA key.
        key: String,
        /// Expected value.
        value: String,
    },
    /// Invert another filter.
    Not(Box<Filter>),
    /// Require all nested filters to match.
    And(Vec<Filter>),
    /// Require any nested filter to match.
    Or(Vec<Filter>),
}

/// Parse a filter expression from CLI args.
#[must_use]
pub fn parse_filter(args: &[&str], today: &str) -> Filter {
    match args {
        [] => Filter::And(Vec::new()),
        [arg] => parse_expression(arg, today),
        _ => Filter::And(
            args.iter()
                .map(|arg| parse_expression(arg, today))
                .collect(),
        ),
    }
}

/// Apply a filter to a single entity.
#[must_use]
pub fn matches(filter: &Filter, entity: &impl Filterable) -> bool {
    match filter {
        Filter::Status(status) => entity.status() == status,
        Filter::Tag(tag) => entity.tags().iter().any(|entity_tag| entity_tag == tag),
        Filter::Priority(priority) => entity.priority() == Some(*priority),
        Filter::Project(project) => entity
            .project_name()
            .is_some_and(|entity_project| entity_project.eq_ignore_ascii_case(project)),
        Filter::HasDeadline => entity.deadline().is_some(),
        Filter::Overdue { today } => entity.deadline().is_some_and(|deadline| deadline < today),
        Filter::Today { today } => {
            entity.deadline().is_some_and(|deadline| deadline <= today)
                || entity
                    .start_date()
                    .is_some_and(|start_date| start_date <= today)
        }
        Filter::Evening => entity.is_evening(),
        Filter::Deleted => entity.is_deleted(),
        Filter::Uda { key, value } => entity.uda_value(key).as_deref() == Some(value.as_str()),
        Filter::Not(inner) => !matches(inner, entity),
        Filter::And(filters) => filters.iter().all(|inner| matches(inner, entity)),
        Filter::Or(filters) => filters.iter().any(|inner| matches(inner, entity)),
    }
}

fn parse_expression(input: &str, today: &str) -> Filter {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return permissive_filter();
    }

    if let Some(parts) = split_case_insensitive(trimmed, " or ") {
        return Filter::Or(
            parts
                .into_iter()
                .map(|part| parse_expression(part, today))
                .collect(),
        );
    }

    if let Some(parts) = split_case_insensitive(trimmed, " and ") {
        return Filter::And(
            parts
                .into_iter()
                .map(|part| parse_expression(part, today))
                .collect(),
        );
    }

    if let Some(rest) = strip_prefix_case_insensitive(trimmed, "not ") {
        return Filter::Not(Box::new(parse_expression(rest, today)));
    }

    parse_atom(trimmed, today)
}

fn parse_atom(input: &str, today: &str) -> Filter {
    if let Some(status) = strip_prefix_case_insensitive(input, "status:") {
        return Filter::Status(status.to_owned());
    }

    if let Some(tag) = strip_prefix_case_insensitive(input, "tag:") {
        return Filter::Tag(tag.to_owned());
    }

    if let Some(tag) = input.strip_prefix('#') {
        return Filter::Tag(tag.to_owned());
    }

    if let Some(priority) = strip_prefix_case_insensitive(input, "priority:") {
        return priority
            .chars()
            .next()
            .map_or_else(permissive_filter, Filter::Priority);
    }

    if let Some(project) = strip_prefix_case_insensitive(input, "project:") {
        return Filter::Project(project.to_owned());
    }

    if input.eq_ignore_ascii_case("+TODAY") {
        return Filter::Today {
            today: today.to_owned(),
        };
    }

    if input.eq_ignore_ascii_case("+OVERDUE") {
        return Filter::Overdue {
            today: today.to_owned(),
        };
    }

    if input.eq_ignore_ascii_case("+EVENING") || input.eq_ignore_ascii_case("evening:true") {
        return Filter::Evening;
    }

    if input.eq_ignore_ascii_case("+DELETED") {
        return Filter::Deleted;
    }

    if input.eq_ignore_ascii_case("+TAGGED") || input.eq_ignore_ascii_case("+DEADLINE") {
        return Filter::HasDeadline;
    }

    if input
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("uda."))
    {
        let rest = &input[4..];
        if let Some((key, value)) = rest.split_once(':') {
            return Filter::Uda {
                key: key.to_owned(),
                value: value.to_owned(),
            };
        }
    }

    permissive_filter()
}

fn strip_prefix_case_insensitive<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    if input.len() < prefix.len() {
        return None;
    }

    let (candidate, remainder) = input.split_at(prefix.len());
    if candidate.eq_ignore_ascii_case(prefix) {
        Some(remainder)
    } else {
        None
    }
}

fn split_case_insensitive<'a>(input: &'a str, needle: &str) -> Option<Vec<&'a str>> {
    let lowercase = input.to_ascii_lowercase();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut search_start = 0;

    while let Some(relative_index) = lowercase[search_start..].find(needle) {
        let index = search_start + relative_index;
        parts.push(input[start..index].trim());
        start = index + needle.len();
        search_start = start;
    }

    if parts.is_empty() {
        None
    } else {
        parts.push(input[start..].trim());
        Some(parts)
    }
}

const fn permissive_filter() -> Filter {
    Filter::And(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::{Filter, Filterable, matches, parse_filter};

    #[derive(Clone, Debug, Default)]
    struct TestEntity {
        status: String,
        tags: Vec<String>,
        priority: Option<char>,
        project_name: Option<String>,
        deadline: Option<String>,
        start_date: Option<String>,
        is_evening: bool,
        is_deleted: bool,
        udas: Vec<(String, String)>,
    }

    impl Filterable for TestEntity {
        fn status(&self) -> &str {
            &self.status
        }

        fn tags(&self) -> &[String] {
            &self.tags
        }

        fn priority(&self) -> Option<char> {
            self.priority
        }

        fn project_name(&self) -> Option<&str> {
            self.project_name.as_deref()
        }

        fn deadline(&self) -> Option<&str> {
            self.deadline.as_deref()
        }

        fn start_date(&self) -> Option<&str> {
            self.start_date.as_deref()
        }

        fn is_evening(&self) -> bool {
            self.is_evening
        }

        fn is_deleted(&self) -> bool {
            self.is_deleted
        }

        fn uda_value(&self, key: &str) -> Option<String> {
            self.udas
                .iter()
                .find(|(candidate, _)| candidate == key)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn status_filter_matches() {
        let entity = TestEntity {
            status: String::from("pending"),
            ..TestEntity::default()
        };

        assert!(matches(
            &parse_filter(&["status:pending"], "2026-06-17"),
            &entity
        ));
    }

    #[test]
    fn tag_filter_matches() {
        let entity = TestEntity {
            tags: vec![String::from("work")],
            ..TestEntity::default()
        };

        assert!(matches(&parse_filter(&["tag:work"], "2026-06-17"), &entity));
    }

    #[test]
    fn tag_filter_does_not_match_missing_tag() {
        let entity = TestEntity {
            tags: vec![String::from("home")],
            ..TestEntity::default()
        };

        assert!(!matches(&parse_filter(&["#work"], "2026-06-17"), &entity));
    }

    #[test]
    fn priority_filter_matches() {
        let entity = TestEntity {
            priority: Some('H'),
            ..TestEntity::default()
        };

        assert!(matches(
            &parse_filter(&["priority:H"], "2026-06-17"),
            &entity
        ));
    }

    #[test]
    fn today_filter_matches_deadline() {
        let entity = TestEntity {
            deadline: Some(String::from("2026-06-17")),
            ..TestEntity::default()
        };

        assert!(matches(&parse_filter(&["+TODAY"], "2026-06-17"), &entity));
    }

    #[test]
    fn overdue_filter_matches_past_deadline() {
        let entity = TestEntity {
            deadline: Some(String::from("2026-06-16")),
            ..TestEntity::default()
        };

        assert!(matches(&parse_filter(&["+OVERDUE"], "2026-06-17"), &entity));
    }

    #[test]
    fn not_filter_inverts_match() {
        let entity = TestEntity {
            status: String::from("done"),
            ..TestEntity::default()
        };

        assert!(!matches(
            &parse_filter(&["not status:done"], "2026-06-17"),
            &entity
        ));
    }

    #[test]
    fn and_filter_requires_all_matches() {
        let entity = TestEntity {
            status: String::from("pending"),
            tags: vec![String::from("work")],
            ..TestEntity::default()
        };
        let filter = Filter::And(vec![
            Filter::Status(String::from("pending")),
            Filter::Tag(String::from("work")),
        ]);

        assert!(matches(&filter, &entity));
    }

    #[test]
    fn multiple_args_create_implicit_and() {
        let filter = parse_filter(&["status:pending", "tag:work"], "2026-06-17");

        assert_eq!(
            filter,
            Filter::And(vec![
                Filter::Status(String::from("pending")),
                Filter::Tag(String::from("work")),
            ])
        );
    }

    #[test]
    fn project_match_is_case_insensitive() {
        let entity = TestEntity {
            project_name: Some(String::from("MyProj")),
            ..TestEntity::default()
        };

        assert!(matches(
            &parse_filter(&["project:myproj"], "2026-06-17"),
            &entity
        ));
    }

    #[test]
    fn unknown_filter_is_permissive() {
        let entity = TestEntity::default();

        assert!(matches(&parse_filter(&["mystery"], "2026-06-17"), &entity));
    }

    #[test]
    fn evening_filter_matches() {
        let entity = TestEntity {
            is_evening: true,
            ..TestEntity::default()
        };

        assert!(matches(&parse_filter(&["+EVENING"], "2026-06-17"), &entity));
    }

    #[test]
    fn deleted_filter_matches() {
        let entity = TestEntity {
            is_deleted: true,
            ..TestEntity::default()
        };

        assert!(matches(&parse_filter(&["+DELETED"], "2026-06-17"), &entity));
    }

    #[test]
    fn or_filter_matches_any_branch() {
        let entity = TestEntity {
            status: String::from("pending"),
            ..TestEntity::default()
        };

        assert!(matches(
            &parse_filter(&["status:done or status:pending"], "2026-06-17"),
            &entity,
        ));
    }

    #[test]
    fn uda_filter_matches_exact_value() {
        let entity = TestEntity {
            udas: vec![(String::from("owner"), String::from("sam"))],
            ..TestEntity::default()
        };

        assert!(matches(
            &parse_filter(&["uda.owner:sam"], "2026-06-17"),
            &entity
        ));
        assert!(!matches(
            &parse_filter(&["uda.owner:alex"], "2026-06-17"),
            &entity,
        ));
    }
}
