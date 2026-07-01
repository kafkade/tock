//! User configuration file (`~/.config/tock/config.toml`) per architecture §7.4.
//!
//! The [`Config`] struct mirrors the documented schema. Every field carries a
//! default, so a missing file — or a file that only overrides a handful of
//! keys — parses cleanly and `tock config show` always renders a full,
//! effective configuration.
//!
//! Precedence (low → high): built-in defaults → config file → environment →
//! CLI flags. The file layer is loaded here; env/flag layering happens at the
//! call sites that already own those inputs.

mod duration;
mod keys;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use keys::{Action, Keymap};

/// The annotated sample configuration written by `tock config init`.
pub const SAMPLE_CONFIG: &str = include_str!("sample.toml");

/// Environment variable that overrides the config file location outright.
const CONFIG_ENV: &str = "TOCK_CONFIG";

/// Top-level configuration, mirroring the §7.4 schema.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// General CLI behaviour.
    pub general: General,
    /// Locale and formatting.
    pub locale: Locale,
    /// Task defaults.
    pub tasks: Tasks,
    /// Urgency scoring coefficients.
    pub urgency: Urgency,
    /// Habit defaults.
    pub habits: Habits,
    /// Time-tracking defaults.
    pub time: Time,
    /// Focus / Pomodoro defaults.
    pub focus: Focus,
    /// Desktop notification settings.
    pub notifications: Notifications,
    /// Named contexts (global saved filters).
    pub contexts: Contexts,
    /// Custom report definitions keyed by name (`[reports.<name>]`).
    pub reports: BTreeMap<String, Report>,
    /// User-defined attribute declarations keyed by key (`[uda.<name>]`).
    pub uda: BTreeMap<String, Uda>,
    /// Hook script settings.
    pub hooks: Hooks,
    /// TUI keybindings.
    pub keys: Keys,
}

/// `[general]`
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    /// View shown by `tock` with no arguments.
    pub default_view: String,
    /// Ask before destructive operations (delete/purge).
    pub confirm_destructive: bool,
    /// Default output format: `table`, `compact`, or `json`.
    pub format: String,
}

impl Default for General {
    fn default() -> Self {
        Self {
            default_view: String::from("today"),
            confirm_destructive: true,
            format: String::from("table"),
        }
    }
}

/// `[locale]`
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Locale {
    /// IANA timezone or `system`.
    pub timezone: String,
    /// `monday`, `sunday`, or `saturday`.
    pub week_starts: String,
    /// `24h` or `12h`.
    pub time_format: String,
    /// Natural-language parser language.
    pub language: String,
}

impl Default for Locale {
    fn default() -> Self {
        Self {
            timezone: String::from("system"),
            week_starts: String::from("monday"),
            time_format: String::from("24h"),
            language: String::from("en"),
        }
    }
}

/// `[tasks]`
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Tasks {
    /// Default priority for new tasks (`none`, `H`, `M`, `L`).
    pub default_priority: String,
    /// Local time when "today" rolls over (`HH:MM`).
    pub auto_today_at: String,
    /// Local time when the evening bucket starts (`HH:MM`).
    pub evening_starts_at: String,
}

impl Default for Tasks {
    fn default() -> Self {
        Self {
            default_priority: String::from("none"),
            auto_today_at: String::from("04:00"),
            evening_starts_at: String::from("18:00"),
        }
    }
}

/// `[urgency]` — maps onto the implemented §2.1.4 weight model.
///
/// The general per-tag-count weight is `tag` (singular); the `[urgency.tags]`
/// sub-table holds additive per-tag overrides. This avoids the TOML key clash
/// in the abbreviated §7.4 dump (which lists both a `tags` scalar and a
/// `[urgency.tags]` table).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Urgency {
    /// Deadline weight.
    pub deadline: f64,
    /// Start-date weight (once available).
    pub start_date: f64,
    /// Priority weight.
    pub priority: f64,
    /// Age weight.
    pub age: f64,
    /// General per-tag-count weight.
    pub tag: f64,
    /// Project-assignment weight.
    pub project: f64,
    /// `+next` boost weight.
    pub next: f64,
    /// Blocked penalty weight.
    pub blocked: f64,
    /// Waiting (future start date) penalty weight.
    pub waiting: f64,
    /// Additive per-tag overrides (`[urgency.tags]`).
    pub tags: BTreeMap<String, f64>,
}

impl Default for Urgency {
    fn default() -> Self {
        let base = tock_core::domain::urgency::UrgencyConfig::default();
        Self {
            deadline: base.deadline_weight,
            start_date: base.start_date_weight,
            priority: base.priority_weight,
            age: base.age_weight,
            tag: base.tag_weight,
            project: base.project_weight,
            next: base.next_weight,
            blocked: base.blocked_weight,
            waiting: base.waiting_weight,
            tags: BTreeMap::new(),
        }
    }
}

impl Urgency {
    /// Convert to the core engine's [`UrgencyConfig`].
    #[must_use]
    pub fn to_core(&self) -> tock_core::domain::urgency::UrgencyConfig {
        tock_core::domain::urgency::UrgencyConfig {
            deadline_weight: self.deadline,
            start_date_weight: self.start_date,
            priority_weight: self.priority,
            age_weight: self.age,
            tag_weight: self.tag,
            project_weight: self.project,
            next_weight: self.next,
            blocked_weight: self.blocked,
            waiting_weight: self.waiting,
            tag_overrides: self.tags.clone(),
        }
    }
}

/// `[habits]`
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Habits {
    /// Missed days tolerated before a streak breaks.
    pub streak_grace_days: u32,
    /// `linear` or `fibonacci`.
    pub level_curve: String,
    /// Prompt for an identity statement on `habit add`.
    pub identity_required: bool,
}

impl Default for Habits {
    fn default() -> Self {
        Self {
            streak_grace_days: 1,
            level_curve: String::from("fibonacci"),
            identity_required: false,
        }
    }
}

/// `[time]`
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Time {
    /// Allow overlapping time blocks.
    pub allow_overlap: bool,
    /// Mark new blocks billable by default.
    pub billable_default: bool,
}

/// `[focus]` — Pomodoro defaults. Durations accept an integer number of
/// minutes or a duration string such as `"25m"` / `"1h"`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Focus {
    /// Work interval, in minutes.
    #[serde(with = "duration::minutes")]
    pub length: u32,
    /// Short break, in minutes.
    #[serde(with = "duration::minutes")]
    pub short_break: u32,
    /// Long break, in minutes.
    #[serde(with = "duration::minutes")]
    pub long_break: u32,
    /// Work cycles before a long break.
    pub cycles: u32,
    /// Automatically start the break after a work interval.
    pub auto_start_break: bool,
}

impl Default for Focus {
    fn default() -> Self {
        let base = tock_core::domain::focus::FocusConfig::default();
        Self {
            length: base.work_minutes,
            short_break: base.short_break_minutes,
            long_break: base.long_break_minutes,
            cycles: base.cycles_before_long_break,
            auto_start_break: true,
        }
    }
}

impl Focus {
    /// Convert to the core [`FocusConfig`].
    #[must_use]
    pub const fn to_core(&self) -> tock_core::domain::focus::FocusConfig {
        tock_core::domain::focus::FocusConfig {
            work_minutes: self.length,
            short_break_minutes: self.short_break,
            long_break_minutes: self.long_break,
            cycles_before_long_break: self.cycles,
        }
    }
}

/// `[notifications]`
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Notifications {
    /// Master switch for desktop notifications.
    pub enabled: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// `[contexts]`
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Contexts {
    /// Currently active context name (empty for none).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub active: String,
    /// Named context filters (`[contexts.<name>]`).
    #[serde(flatten)]
    pub definitions: BTreeMap<String, ContextDef>,
}

/// A named context (`[contexts.<name>]`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContextDef {
    /// Implicit filter applied while the context is active.
    pub filter: String,
}

/// A custom report (`[reports.<name>]`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Report {
    /// Human-readable description.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Filter expression.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub filter: String,
    /// Columns to show.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    /// Sort keys.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<String>,
    /// Optional group-by field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>,
    /// Optional output format override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

/// A user-defined attribute declaration (`[uda.<name>]`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)] // `type` is a reserved word; `uda_type` reads clearly
pub struct Uda {
    /// UDA type: `string`, `int`/`number`, `float`, `bool`, `enum`,
    /// `duration`, `date`, `task_ref`.
    #[serde(rename = "type", default = "default_uda_type")]
    pub uda_type: String,
    /// Optional human-readable label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Optional default value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Allowed values for `enum` types.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
}

fn default_uda_type() -> String {
    String::from("string")
}

/// `[hooks]`
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Hooks {
    /// Directory containing hook scripts.
    pub dir: String,
    /// Hook timeout (e.g. `"5s"`).
    pub timeout: String,
}

impl Default for Hooks {
    fn default() -> Self {
        Self {
            dir: String::from("~/.config/tock/hooks"),
            timeout: String::from("5s"),
        }
    }
}

/// `[keys]` — TUI keybindings. Each value is a single character or a named
/// key (`tab`, `backtab`, `enter`, `esc`, `up`, `down`, `left`, `right`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Keys {
    /// Quit the TUI.
    pub quit: String,
    /// Focus the next pane.
    pub next_pane: String,
    /// Focus the previous pane.
    pub prev_pane: String,
    /// Move the selection down.
    pub down: String,
    /// Move the selection up.
    pub up: String,
    /// Jump to the top.
    pub top: String,
    /// Jump to the bottom.
    pub bottom: String,
    /// Activate the selected item.
    pub activate: String,
    /// Complete the selected task.
    pub complete: String,
    /// Delete the selected task.
    pub delete: String,
    /// Reload tasks.
    pub reload: String,
    /// Toggle the help overlay.
    pub help: String,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            quit: String::from("q"),
            next_pane: String::from("tab"),
            prev_pane: String::from("backtab"),
            down: String::from("j"),
            up: String::from("k"),
            top: String::from("g"),
            bottom: String::from("G"),
            activate: String::from("enter"),
            complete: String::from("d"),
            delete: String::from("x"),
            reload: String::from("r"),
            help: String::from("?"),
        }
    }
}

/// Resolve the config file path, honoring `TOCK_CONFIG`, `XDG_CONFIG_HOME`,
/// and (on macOS) the `~/.config` dotfile path for parity with §7.4.
#[must_use]
pub fn resolve_path(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    if let Some(path) = std::env::var_os(CONFIG_ENV) {
        return Some(PathBuf::from(path));
    }
    // Prefer the XDG dotfile path (default `~/.config/tock/config.toml`), which
    // §7.4 honors on every platform.
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(dir).join("tock").join("config.toml"));
    }
    if let Some(home) = dirs::home_dir() {
        let dotfile = home.join(".config").join("tock").join("config.toml");
        if dotfile.exists() {
            return Some(dotfile);
        }
    }
    dirs::config_dir().map(|dir| dir.join("tock").join("config.toml"))
}

/// Load configuration, layering an existing file over the defaults.
///
/// Returns the effective [`Config`] and the path that was consulted (whether or
/// not it exists). A missing file yields all defaults.
///
/// # Errors
/// Returns an error if the file exists but cannot be read or parsed.
pub fn load(explicit: Option<&Path>) -> Result<(Config, Option<PathBuf>), ConfigError> {
    let path = resolve_path(explicit);
    let Some(path) = path else {
        return Ok((Config::default(), None));
    };
    if !path.exists() {
        return Ok((Config::default(), Some(path)));
    }
    let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;
    let config = parse(&text).map_err(|source| ConfigError::Parse {
        path: path.clone(),
        source,
    })?;
    Ok((config, Some(path)))
}

/// Parse a config from TOML text.
///
/// # Errors
/// Returns a [`toml::de::Error`] on malformed input.
pub fn parse(text: &str) -> Result<Config, toml::de::Error> {
    toml::from_str(text)
}

/// Render the effective config as pretty TOML.
///
/// # Errors
/// Returns a [`toml::ser::Error`] if serialization fails.
pub fn to_toml(config: &Config) -> Result<String, toml::ser::Error> {
    toml::to_string_pretty(config)
}

/// Errors from loading the config file.
#[derive(Debug)]
pub enum ConfigError {
    /// The file could not be read.
    Read {
        /// The offending path.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The file could not be parsed.
    Parse {
        /// The offending path.
        path: PathBuf,
        /// Underlying TOML error.
        source: toml::de::Error,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "cannot read config {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(f, "invalid config {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::float_cmp)]

    use super::*;

    #[test]
    fn empty_input_yields_defaults() {
        let config = parse("").unwrap();
        assert_eq!(config.general.default_view, "today");
        assert_eq!(config.urgency.deadline, 1.0);
        assert_eq!(config.focus.length, 25);
        assert_eq!(config.keys.complete, "d");
        assert!(config.notifications.enabled);
    }

    #[test]
    fn partial_file_overrides_only_named_keys() {
        let config = parse(
            r#"
            [general]
            default_view = "inbox"

            [urgency]
            deadline = 9.0
            "#,
        )
        .unwrap();
        // Overridden.
        assert_eq!(config.general.default_view, "inbox");
        assert_eq!(config.urgency.deadline, 9.0);
        // Untouched defaults preserved.
        assert!(config.general.confirm_destructive);
        assert_eq!(config.urgency.priority, 0.6);
    }

    #[test]
    fn urgency_maps_to_core_with_tag_overrides() {
        let config = parse(
            r"
            [urgency]
            next = 15.0
            [urgency.tags]
            urgent = 4.0
            someday = -3.0
            ",
        )
        .unwrap();
        let core = config.urgency.to_core();
        assert_eq!(core.next_weight, 15.0);
        assert_eq!(core.tag_overrides.get("urgent"), Some(&4.0));
        assert_eq!(core.tag_overrides.get("someday"), Some(&-3.0));
    }

    #[test]
    fn focus_accepts_integer_or_duration_string() {
        let config = parse(
            r#"
            [focus]
            length = "50m"
            short_break = 8
            long_break = "1h"
            cycles = 3
            "#,
        )
        .unwrap();
        assert_eq!(config.focus.length, 50);
        assert_eq!(config.focus.short_break, 8);
        assert_eq!(config.focus.long_break, 60);
        let core = config.focus.to_core();
        assert_eq!(core.work_minutes, 50);
        assert_eq!(core.cycles_before_long_break, 3);
    }

    #[test]
    fn parses_uda_and_context_and_report_tables() {
        let config = parse(
            r#"
            [contexts]
            active = "work"
            [contexts.work]
            filter = "+work -archived"

            [uda.effort]
            type = "int"
            label = "Effort (1-5)"

            [reports.standup]
            description = "Yesterday + today"
            filter = "+TODAY"
            columns = ["sid", "description"]
            "#,
        )
        .unwrap();
        assert_eq!(config.contexts.active, "work");
        assert_eq!(
            config
                .contexts
                .definitions
                .get("work")
                .map(|c| c.filter.as_str()),
            Some("+work -archived")
        );
        assert_eq!(
            config.uda.get("effort").map(|u| u.uda_type.as_str()),
            Some("int")
        );
        assert_eq!(
            config.reports.get("standup").map(|r| r.columns.len()),
            Some(2)
        );
    }

    #[test]
    fn sample_config_parses() {
        let config = parse(SAMPLE_CONFIG).unwrap();
        // Round-trips through the serializer too.
        let rendered = to_toml(&config).unwrap();
        assert!(parse(&rendered).is_ok());
    }
}
