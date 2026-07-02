//! Markdown export — renders domain data via Tera templates.
//!
//! Provides three built-in templates (task list, habit report, time report)
//! and supports user-supplied custom templates. All templates receive an
//! [`ExportContext`] with tasks, habits, time blocks, and projects.

use std::collections::HashMap;

use rusqlite::Connection;
use serde::Serialize;
use time::format_description::well_known::Rfc3339;

use tock_core::domain::habit::Habit;
use tock_core::domain::project::Project;
use tock_core::domain::task::Task;
use tock_core::domain::time_block::TimeBlock;

use crate::error::Error;

// ---------------------------------------------------------------------------
// Built-in template sources
// ---------------------------------------------------------------------------

/// Task list template matching architecture §9.6.
const TASK_LIST_TEMPLATE: &str = r#"# Today — {{ today }}

{% for group in tasks_by_project -%}
## {{ group.name }}

{% for t in group.tasks -%}
- [{% if t.status == "done" %}x{% else %} {% endif %}] **{{ t.title }}**
{%- if t.deadline %} _due {{ t.deadline }}_{% endif %}
{%- if t.tags | length > 0 %} {% for tag in t.tags %}`#{{ tag }}` {% endfor %}{% endif %}
{%- if t.priority %} ({{ t.priority }}){% endif %}
{% if t.annotations | length > 0 %}{% for a in t.annotations %}  - _{{ a.created_at }}_ — {{ a.body }}
{% endfor %}{% endif %}
{%- endfor %}
{% endfor -%}
"#;

/// Habit weekly report template matching architecture §9.6.
const HABIT_REPORT_TEMPLATE: &str = r"# Habits Report — {{ today }}

| Habit | Identity | Direction | Streak | Best | Level |
|-------|----------|-----------|--------|------|-------|
{% for h in habits -%}
| {{ h.title }} | {{ h.identity }} | {{ h.direction }} | 🔥 {{ h.streak_current }} | {{ h.streak_best }} | {{ h.level_name }} ({{ h.level }}) |
{% endfor -%}
";

/// Time report template matching architecture §9.6.
const TIME_REPORT_TEMPLATE: &str = r"# Time Report — {{ today }}

**Total:** {{ total_duration | duration }}  ·  **Billable:** {{ billable_duration | duration }}

| # | Task | Project | Duration | Billable |
|---|------|---------|----------|----------|
{% for tb in time_blocks -%}
| {{ tb.sid }} | {{ tb.title }} | {{ tb.project_name }} | {{ tb.duration_secs | duration }} | {% if tb.billable %}✓{% else %}—{% endif %} |
{% endfor -%}
";

// ---------------------------------------------------------------------------
// Serializable context types
// ---------------------------------------------------------------------------

/// An annotation prepared for template rendering.
#[derive(Clone, Debug, Serialize)]
pub struct AnnotationContext {
    /// Annotation text.
    pub body: String,
    /// ISO timestamp.
    pub created_at: String,
}

/// A task prepared for template rendering.
#[derive(Clone, Debug, Serialize)]
pub struct TaskContext {
    /// Short workspace-local ID.
    pub sid: u32,
    /// Task title.
    pub title: String,
    /// Status string (inbox, pending, started, done, cancelled, someday).
    pub status: String,
    /// Priority character (H, M, L) or empty.
    pub priority: Option<String>,
    /// Deadline (ISO date string).
    pub deadline: Option<String>,
    /// Start date (ISO date string).
    pub start_date: Option<String>,
    /// Tag list.
    pub tags: Vec<String>,
    /// Notes.
    pub notes: Option<String>,
    /// Whether this is an evening task.
    pub evening: bool,
    /// Resolved project name (empty string if no project).
    pub project_name: String,
    /// Urgency score.
    pub urgency: f64,
    /// ISO timestamp.
    pub created_at: String,
    /// ISO timestamp.
    pub modified_at: String,
    /// ISO timestamp.
    pub done_at: Option<String>,
    /// ISO timestamp.
    pub cancelled_at: Option<String>,
    /// Append-only annotations, oldest first.
    pub annotations: Vec<AnnotationContext>,
}

/// A habit prepared for template rendering.
#[derive(Clone, Debug, Serialize)]
pub struct HabitContext {
    /// Short workspace-local ID.
    pub sid: u32,
    /// Habit title.
    pub title: String,
    /// Identity statement or empty.
    pub identity: String,
    /// "build" or "break".
    pub direction: String,
    /// Current streak.
    pub streak_current: u32,
    /// Best streak.
    pub streak_best: u32,
    /// Progression level number.
    pub level: u32,
    /// Level display name.
    pub level_name: String,
    /// Total XP.
    pub xp: u32,
    /// Cadence description.
    pub cadence: String,
    /// ISO timestamp.
    pub created_at: String,
}

/// A time block prepared for template rendering.
#[derive(Clone, Debug, Serialize)]
pub struct TimeBlockContext {
    /// Short workspace-local ID.
    pub sid: u32,
    /// Block title.
    pub title: String,
    /// ISO timestamp.
    pub start_ts: String,
    /// ISO timestamp (empty if still running).
    pub end_ts: Option<String>,
    /// Duration in seconds (0 if still running).
    pub duration_secs: i64,
    /// Resolved project name.
    pub project_name: String,
    /// Whether billable.
    pub billable: bool,
    /// Source type.
    pub source: String,
    /// Notes.
    pub notes: Option<String>,
}

/// A project prepared for template rendering.
#[derive(Clone, Debug, Serialize)]
pub struct ProjectContext {
    /// Short workspace-local ID.
    pub sid: u32,
    /// Project name.
    pub name: String,
    /// Status string.
    pub status: String,
    /// Optional deadline.
    pub deadline: Option<String>,
}

/// A group of tasks under a project name, for template iteration.
#[derive(Clone, Debug, Serialize)]
pub struct TaskGroup {
    /// Project name (or "(no project)").
    pub name: String,
    /// Tasks in this group.
    pub tasks: Vec<TaskContext>,
}

/// Top-level context passed to every Markdown template.
#[derive(Clone, Debug, Serialize)]
pub struct ExportContext {
    /// Today's date as a human-readable string.
    pub today: String,
    /// All tasks (post-filter).
    pub tasks: Vec<TaskContext>,
    /// Tasks grouped by project name.
    pub tasks_by_project: Vec<TaskGroup>,
    /// All habits.
    pub habits: Vec<HabitContext>,
    /// All time blocks (within requested range).
    pub time_blocks: Vec<TimeBlockContext>,
    /// All projects.
    pub projects: Vec<ProjectContext>,
    /// Total tracked duration in seconds.
    pub total_duration: i64,
    /// Billable tracked duration in seconds.
    pub billable_duration: i64,
}

// ---------------------------------------------------------------------------
// Built-in template enum
// ---------------------------------------------------------------------------

/// Available built-in Markdown templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinTemplate {
    /// Task list grouped by project.
    TaskList,
    /// Habit tracking report.
    HabitReport,
    /// Time tracking report.
    TimeReport,
}

impl BuiltinTemplate {
    /// Parse from a CLI string value.
    ///
    /// # Errors
    /// Returns [`Error::UnknownTemplate`] if the string doesn't match.
    pub fn parse_name(s: &str) -> Result<Self, Error> {
        match s {
            "task-list" | "tasks" => Ok(Self::TaskList),
            "habit-report" | "habits" => Ok(Self::HabitReport),
            "time-report" | "time" => Ok(Self::TimeReport),
            _ => Err(Error::UnknownTemplate(s.to_string())),
        }
    }

    /// Template source string for this built-in.
    const fn source(self) -> &'static str {
        match self {
            Self::TaskList => TASK_LIST_TEMPLATE,
            Self::HabitReport => HABIT_REPORT_TEMPLATE,
            Self::TimeReport => TIME_REPORT_TEMPLATE,
        }
    }
}

// ---------------------------------------------------------------------------
// Template selection
// ---------------------------------------------------------------------------

/// Which template to render.
pub enum TemplateSource<'a> {
    /// A built-in template.
    Builtin(BuiltinTemplate),
    /// A user-supplied template string.
    Custom(&'a str),
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn format_ts(ts: &time::OffsetDateTime) -> String {
    ts.format(&Rfc3339).unwrap_or_default()
}

/// Build a project-ID → name lookup from a project list.
fn project_lookup(projects: &[Project]) -> HashMap<uuid::Uuid, String> {
    projects.iter().map(|p| (p.id, p.name.clone())).collect()
}

/// Convert a [`Task`] to a [`TaskContext`].
fn task_to_context(task: &Task, project_map: &HashMap<uuid::Uuid, String>) -> TaskContext {
    TaskContext {
        sid: task.sid,
        title: task.title.clone(),
        status: task.status.as_str().to_string(),
        priority: task.priority.as_ref().map(|p| p.as_char().to_string()),
        deadline: task.deadline.clone(),
        start_date: task.start_date.clone(),
        tags: task.tags.clone(),
        notes: task.notes.clone(),
        evening: task.evening,
        project_name: task
            .project_id
            .and_then(|id| project_map.get(&id).cloned())
            .unwrap_or_default(),
        urgency: task.urgency,
        created_at: format_ts(&task.created_at),
        modified_at: format_ts(&task.modified_at),
        done_at: task.done_at.as_ref().map(format_ts),
        cancelled_at: task.cancelled_at.as_ref().map(format_ts),
        annotations: Vec::new(),
    }
}

/// Convert a [`Habit`] to a [`HabitContext`].
fn habit_to_context(habit: &Habit) -> HabitContext {
    HabitContext {
        sid: habit.sid,
        title: habit.title.clone(),
        identity: habit.identity.clone().unwrap_or_default(),
        direction: habit.direction.as_str().to_string(),
        streak_current: habit.streak_current,
        streak_best: habit.streak_best,
        level: habit.level,
        level_name: habit.level_name().to_string(),
        xp: habit.xp,
        cadence: habit.cadence.clone(),
        created_at: format_ts(&habit.created_at),
    }
}

/// Convert a [`TimeBlock`] to a [`TimeBlockContext`].
fn time_block_to_context(
    tb: &TimeBlock,
    project_map: &HashMap<uuid::Uuid, String>,
) -> TimeBlockContext {
    let duration_secs = tb.duration().map_or(0, time::Duration::whole_seconds);
    TimeBlockContext {
        sid: tb.sid,
        title: tb.title.clone(),
        start_ts: format_ts(&tb.start_ts),
        end_ts: tb.end_ts.as_ref().map(format_ts),
        duration_secs,
        project_name: tb
            .project_id
            .and_then(|id| project_map.get(&id).cloned())
            .unwrap_or_default(),
        billable: tb.billable,
        source: tb.source.as_str().to_string(),
        notes: tb.notes.clone(),
    }
}

/// Convert a [`Project`] to a [`ProjectContext`].
fn project_to_context(project: &Project) -> ProjectContext {
    ProjectContext {
        sid: project.sid,
        name: project.name.clone(),
        status: project.status.as_str().to_string(),
        deadline: project.deadline.clone(),
    }
}

/// Group tasks by project name, sorted alphabetically. Tasks with no project
/// are grouped under "(no project)".
fn group_tasks_by_project(tasks: &[TaskContext]) -> Vec<TaskGroup> {
    let mut groups: std::collections::BTreeMap<String, Vec<TaskContext>> =
        std::collections::BTreeMap::new();
    for t in tasks {
        let key = if t.project_name.is_empty() {
            "(no project)".to_string()
        } else {
            t.project_name.clone()
        };
        groups.entry(key).or_default().push(t.clone());
    }
    groups
        .into_iter()
        .map(|(name, tasks)| TaskGroup { name, tasks })
        .collect()
}

// ---------------------------------------------------------------------------
// Context builder
// ---------------------------------------------------------------------------

/// Build an [`ExportContext`] from domain objects.
#[must_use]
pub fn build_context(
    tasks: &[Task],
    habits: &[Habit],
    time_blocks: &[TimeBlock],
    projects: &[Project],
) -> ExportContext {
    let project_map = project_lookup(projects);

    let task_contexts: Vec<TaskContext> = tasks
        .iter()
        .map(|t| task_to_context(t, &project_map))
        .collect();

    let tasks_by_project = group_tasks_by_project(&task_contexts);

    let tb_contexts: Vec<TimeBlockContext> = time_blocks
        .iter()
        .map(|tb| time_block_to_context(tb, &project_map))
        .collect();

    let total_duration: i64 = tb_contexts.iter().map(|tb| tb.duration_secs).sum();
    let billable_duration: i64 = tb_contexts
        .iter()
        .filter(|tb| tb.billable)
        .map(|tb| tb.duration_secs)
        .sum();

    let today = time::OffsetDateTime::now_utc().date().to_string();

    ExportContext {
        today,
        tasks: task_contexts,
        tasks_by_project,
        habits: habits.iter().map(habit_to_context).collect(),
        time_blocks: tb_contexts,
        projects: projects.iter().map(project_to_context).collect(),
        total_duration,
        billable_duration,
    }
}

// ---------------------------------------------------------------------------
// Custom Tera filter: duration
// ---------------------------------------------------------------------------

/// Tera filter that converts seconds (integer) to a human-readable duration
/// string like "2h 15m" or "45m 30s".
#[allow(clippy::unnecessary_wraps)] // Tera filter API requires Result return type
fn duration_filter(
    value: &tera::Value,
    _kwargs: tera::Kwargs,
    _state: &tera::State<'_>,
) -> tera::TeraResult<tera::Value> {
    #[allow(clippy::cast_possible_truncation)] // duration seconds fit in i64
    let secs = value
        .as_i64()
        .or_else(|| value.as_f64().map(|f| f as i64))
        .unwrap_or(0);

    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    let result = if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    };

    Ok(tera::Value::from(result))
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// Render Markdown from an [`ExportContext`] using the specified template.
///
/// # Errors
/// Returns [`Error::Template`] if the template is invalid or rendering fails.
pub fn render_markdown(
    context: &ExportContext,
    template: &TemplateSource<'_>,
) -> Result<String, Error> {
    let mut tera = tera::Tera::default();
    tera.register_filter("duration", duration_filter);

    let template_name = "export";
    let source = match template {
        TemplateSource::Builtin(builtin) => builtin.source(),
        TemplateSource::Custom(src) => src,
    };
    tera.add_raw_template(template_name, source)?;

    let tera_context = tera::Context::from_serialize(context)?;
    let rendered = tera.render(template_name, &tera_context)?;
    Ok(rendered)
}

// ---------------------------------------------------------------------------
// Convenience: load from Connection and render
// ---------------------------------------------------------------------------

/// Populate task annotations in an already-built [`ExportContext`].
///
/// Kept separate from [`build_context`] so the latter stays pure (no I/O).
/// Annotations are matched to tasks by `sid`.
///
/// # Errors
/// Returns errors from storage queries.
pub fn attach_task_annotations(
    conn: &Connection,
    tasks: &[Task],
    context: &mut ExportContext,
) -> Result<(), Error> {
    let mut by_sid: HashMap<u32, Vec<AnnotationContext>> = HashMap::new();
    for task in tasks {
        let annotations = tock_storage::repo::annotation_repo::list_for_entity(
            conn,
            task.id,
            tock_core::domain::annotation::ENTITY_KIND_TASK,
        )?
        .iter()
        .map(|annotation| AnnotationContext {
            body: annotation.body.clone(),
            created_at: format_ts(&annotation.created_at),
        })
        .collect();
        by_sid.insert(task.sid, annotations);
    }

    for task_ctx in &mut context.tasks {
        if let Some(annotations) = by_sid.get(&task_ctx.sid) {
            task_ctx.annotations.clone_from(annotations);
        }
    }
    for group in &mut context.tasks_by_project {
        for task_ctx in &mut group.tasks {
            if let Some(annotations) = by_sid.get(&task_ctx.sid) {
                task_ctx.annotations.clone_from(annotations);
            }
        }
    }
    Ok(())
}

/// Load all domain data from the vault and render a Markdown export.
///
/// # Errors
/// Returns errors from storage queries or template rendering.
pub fn export_markdown(conn: &Connection, template: &TemplateSource<'_>) -> Result<String, Error> {
    let tasks = tock_storage::repo::task_repo::list(conn, false)?;
    let habits = tock_storage::repo::habit_repo::list(conn, false)?;
    let time_blocks = tock_storage::repo::time_block_repo::list(conn, true)?;
    let projects = tock_storage::repo::project_repo::list(conn, false)?;

    let mut context = build_context(&tasks, &habits, &time_blocks, &projects);
    attach_task_annotations(conn, &tasks, &mut context)?;
    render_markdown(&context, template)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::panic)] // Tests use panic via assert macros
mod tests {
    use super::*;
    use tock_core::domain::habit::HabitDirection;
    use tock_core::domain::project::ProjectStatus;
    use tock_core::domain::task::{Priority, TaskStatus};
    use tock_core::domain::time_block::BlockSource;
    use tock_core::domain::uda::UdaValues;

    fn sample_project() -> Project {
        Project {
            id: uuid::Uuid::new_v4(),
            sid: 1,
            name: "MyProject".to_string(),
            notes: None,
            status: ProjectStatus::Active,
            area_id: None,
            deadline: None,
            sort_order: 0,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            modified_at: time::OffsetDateTime::UNIX_EPOCH,
            archived_at: None,
        }
    }

    fn sample_task(project_id: Option<uuid::Uuid>) -> Task {
        Task {
            id: uuid::Uuid::new_v4(),
            sid: 1,
            title: "Write tests".to_string(),
            notes: None,
            status: TaskStatus::Pending,
            area_id: None,
            project_id,
            heading_id: None,
            parent_id: None,
            start_date: None,
            deadline: Some("2026-06-01".to_string()),
            scheduled_for: None,
            recurrence: None,
            priority: Some(Priority::High),
            evening: false,
            udas: UdaValues::default(),
            tags: vec!["dev".to_string(), "urgent".to_string()],
            depends_on: vec![],
            checklist: vec![],
            urgency: 8.5,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            modified_at: time::OffsetDateTime::UNIX_EPOCH,
            done_at: None,
            cancelled_at: None,
            deleted_at: None,
        }
    }

    fn sample_habit() -> Habit {
        Habit {
            id: uuid::Uuid::new_v4(),
            sid: 1,
            title: "Exercise".to_string(),
            identity: Some("I am someone who exercises daily".to_string()),
            cue: None,
            craving: None,
            response: None,
            reward: None,
            direction: HabitDirection::Build,
            cadence: "daily".to_string(),
            minimum: "boolean".to_string(),
            stack_after: None,
            stack_delay_s: 0,
            area_id: None,
            project_id: None,
            level: 3,
            xp: 150,
            streak_current: 12,
            streak_best: 30,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            modified_at: time::OffsetDateTime::UNIX_EPOCH,
            archived_at: None,
        }
    }

    fn sample_time_block(project_id: Option<uuid::Uuid>) -> TimeBlock {
        let start = time::OffsetDateTime::UNIX_EPOCH;
        let end = start + time::Duration::seconds(5400); // 1h30m
        TimeBlock {
            id: uuid::Uuid::new_v4(),
            sid: 1,
            title: "Deep work".to_string(),
            start_ts: start,
            end_ts: Some(end),
            project_id,
            task_id: None,
            notes: None,
            source: BlockSource::Timer,
            billable: true,
            created_at: start,
            modified_at: start,
        }
    }

    /// Helper to render and assert success, returning the Markdown string.
    fn render_ok(ctx: &ExportContext, tpl: &TemplateSource<'_>) -> String {
        let result = render_markdown(ctx, tpl);
        assert!(result.is_ok(), "render failed: {:?}", result.err());
        result.unwrap_or_default()
    }

    #[test]
    fn task_list_template_renders() {
        let proj = sample_project();
        let task = sample_task(Some(proj.id));
        let ctx = build_context(&[task], &[], &[], &[proj]);
        let md = render_ok(&ctx, &TemplateSource::Builtin(BuiltinTemplate::TaskList));
        assert!(md.contains("# Today"));
        assert!(md.contains("## MyProject"));
        assert!(md.contains("**Write tests**"));
        assert!(md.contains("_due 2026-06-01_"));
        assert!(md.contains("`#dev`"));
        assert!(md.contains("(H)"));
    }

    #[test]
    fn task_list_no_project() {
        let task = sample_task(None);
        let ctx = build_context(&[task], &[], &[], &[]);
        let md = render_ok(&ctx, &TemplateSource::Builtin(BuiltinTemplate::TaskList));
        assert!(md.contains("## (no project)"));
    }

    #[test]
    fn task_list_renders_annotations() {
        let task = sample_task(None);
        let mut ctx = build_context(&[task], &[], &[], &[]);
        let annotation = AnnotationContext {
            body: "reviewed with team".to_string(),
            created_at: "2026-01-02".to_string(),
        };
        for t in &mut ctx.tasks {
            t.annotations.push(annotation.clone());
        }
        for group in &mut ctx.tasks_by_project {
            for t in &mut group.tasks {
                t.annotations.push(annotation.clone());
            }
        }
        let md = render_ok(&ctx, &TemplateSource::Builtin(BuiltinTemplate::TaskList));
        assert!(
            md.contains("reviewed with team"),
            "missing annotation: {md}"
        );
        assert!(md.contains("_2026-01-02_"), "missing annotation date: {md}");
    }

    #[test]
    fn habit_report_template_renders() {
        let habit = sample_habit();
        let ctx = build_context(&[], &[habit], &[], &[]);
        let md = render_ok(&ctx, &TemplateSource::Builtin(BuiltinTemplate::HabitReport));
        assert!(md.contains("# Habits Report"));
        assert!(md.contains("Exercise"));
        assert!(md.contains("🔥 12"));
        assert!(md.contains("Established"));
    }

    #[test]
    fn time_report_template_renders() {
        let proj = sample_project();
        let tb = sample_time_block(Some(proj.id));
        let ctx = build_context(&[], &[], &[tb], &[proj]);
        let md = render_ok(&ctx, &TemplateSource::Builtin(BuiltinTemplate::TimeReport));
        assert!(md.contains("# Time Report"));
        assert!(md.contains("Deep work"));
        assert!(md.contains("MyProject"));
        assert!(md.contains("1h 30m")); // duration filter
        assert!(md.contains("✓")); // billable
    }

    #[test]
    fn custom_template_renders() {
        let task = sample_task(None);
        let custom =
            "Tasks: {{ tasks | length }}\n{% for t in tasks %}* {{ t.title }}\n{% endfor %}";
        let ctx = build_context(&[task], &[], &[], &[]);
        let md = render_ok(&ctx, &TemplateSource::Custom(custom));
        assert!(md.contains("Tasks: 1"));
        assert!(md.contains("* Write tests"));
    }

    #[test]
    fn duration_filter_formats_hours() {
        let kwargs = tera::Kwargs::default();
        let context = tera::Context::new();
        let state = tera::State::new(&context);
        let val = tera::Value::from(7380_i64); // 2h3m
        let result = duration_filter(&val, kwargs, &state);
        assert_eq!(result.ok(), Some(tera::Value::from("2h 03m")));
    }

    #[test]
    fn duration_filter_formats_minutes() {
        let kwargs = tera::Kwargs::default();
        let context = tera::Context::new();
        let state = tera::State::new(&context);
        let val = tera::Value::from(125_i64); // 2m5s
        let result = duration_filter(&val, kwargs, &state);
        assert_eq!(result.ok(), Some(tera::Value::from("2m 05s")));
    }

    #[test]
    fn duration_filter_formats_seconds() {
        let kwargs = tera::Kwargs::default();
        let context = tera::Context::new();
        let state = tera::State::new(&context);
        let val = tera::Value::from(42_i64);
        let result = duration_filter(&val, kwargs, &state);
        assert_eq!(result.ok(), Some(tera::Value::from("42s")));
    }

    #[test]
    fn builtin_template_parse_name_valid() {
        assert_eq!(
            BuiltinTemplate::parse_name("task-list").ok(),
            Some(BuiltinTemplate::TaskList),
        );
        assert_eq!(
            BuiltinTemplate::parse_name("habits").ok(),
            Some(BuiltinTemplate::HabitReport),
        );
        assert_eq!(
            BuiltinTemplate::parse_name("time").ok(),
            Some(BuiltinTemplate::TimeReport),
        );
    }

    #[test]
    fn builtin_template_parse_name_invalid() {
        assert!(BuiltinTemplate::parse_name("nonexistent").is_err());
    }

    #[test]
    fn empty_context_renders() {
        let ctx = build_context(&[], &[], &[], &[]);
        let result = render_markdown(&ctx, &TemplateSource::Builtin(BuiltinTemplate::TaskList));
        assert!(result.is_ok());
    }

    #[test]
    fn total_and_billable_durations_computed() {
        let proj = sample_project();
        let mut tb1 = sample_time_block(Some(proj.id));
        tb1.billable = true;
        let mut tb2 = sample_time_block(Some(proj.id));
        tb2.sid = 2;
        tb2.billable = false;

        let ctx = build_context(&[], &[], &[tb1, tb2], &[proj]);
        assert_eq!(ctx.total_duration, 10800); // 2 × 5400
        assert_eq!(ctx.billable_duration, 5400); // only first
    }

    #[test]
    fn done_task_renders_checkbox() {
        let mut task = sample_task(None);
        task.status = TaskStatus::Done;
        task.done_at = Some(time::OffsetDateTime::UNIX_EPOCH);
        let ctx = build_context(&[task], &[], &[], &[]);
        let md = render_ok(&ctx, &TemplateSource::Builtin(BuiltinTemplate::TaskList));
        assert!(md.contains("[x]"));
    }
}
