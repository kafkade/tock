//! `tock ls` — list tasks with optional filtering.

/// Basic filter parsed from CLI arguments.
#[derive(Debug, Default)]
pub struct ListFilter {
    /// Filter by status (e.g. `status:pending`).
    pub status: Option<String>,
    /// Filter by project name (e.g. `project:myproj`).
    pub project: Option<String>,
    /// Filter by tag name (e.g. `tag:errands`).
    pub tag: Option<String>,
    /// Filter by priority (e.g. `priority:H`).
    pub priority: Option<String>,
}

/// Parse filter arguments from the CLI.
#[must_use]
pub fn parse_filter(args: &[String]) -> ListFilter {
    let mut f = ListFilter::default();
    for arg in args {
        if let Some(val) = arg.strip_prefix("status:") {
            f.status = Some(val.to_string());
        } else if let Some(val) = arg.strip_prefix("project:") {
            f.project = Some(val.to_string());
        } else if let Some(val) = arg.strip_prefix("tag:") {
            f.tag = Some(val.to_string());
        } else if let Some(val) = arg.strip_prefix("priority:") {
            f.priority = Some(val.to_string());
        }
    }
    f
}
