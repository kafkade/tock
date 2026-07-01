//! CSV import — reads CSV files with flexible column mapping.
//!
//! Supports two modes:
//!
//! 1. **Auto-detection**: header names are matched against a built-in
//!    dictionary of common column names (case- and punctuation-insensitive).
//!
//! 2. **Explicit mapping**: a TOML file specifies which CSV columns map to
//!    which tock fields, with optional date formats, tag separators, and
//!    typed UDA declarations.
//!
//! ## Auto-detected column aliases
//!
//! | CSV header aliases                       | Tock field   |
//! |------------------------------------------|--------------|
//! | description, title, task, name           | title        |
//! | due, duedate, deadline                   | deadline     |
//! | start, startdate, defer, scheduled       | `start_date` |
//! | project, list                            | project      |
//! | tags, labels, categories                 | tags         |
//! | priority, pri                            | priority     |
//! | status, state, completed                 | status       |
//! | notes, note, description2                | notes        |
//!
//! Unmatched columns are imported as UDAs with string type.
//!
//! ## TOML mapping format
//!
//! ```toml
//! [columns]
//! "Task Name"   = "description"
//! "Due"         = { field = "due_date", format = "%m/%d/%Y" }
//! "List"        = "project"
//! "Tags"        = { field = "tags", split = "," }
//! "Effort"      = { uda = "effort", type = "number" }
//!
//! [defaults]
//! status = "pending"
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use rusqlite::Connection;
use tock_core::domain::project::NewProject;
use tock_core::domain::task::{NewTask, Priority, TaskStatus};
use tock_core::domain::uda::{UdaDefinition, UdaType, UdaValues};

/// Built-in alias dictionary: (aliases, target field).
const ALIAS_TABLE: &[(&[&str], &str)] = &[
    (&["description", "title", "task", "name"], "title"),
    (&["due", "duedate", "deadline"], "due_date"),
    (&["start", "startdate", "defer", "scheduled"], "start_date"),
    (&["project", "list"], "project"),
    (&["tags", "labels", "categories"], "tags"),
    (&["priority", "pri"], "priority"),
    (&["status", "state", "completed"], "status"),
    (&["notes", "note", "description2"], "notes"),
];

/// Fields that auto-detection recognizes but doesn't import (warns instead).
const RECOGNIZED_BUT_SKIPPED: &[&str] = &["area", "folder", "category"];

// ── Mapping types ─────────────────────────────────────────────────

/// What a single CSV column maps to in tock.
#[derive(Clone, Debug)]
enum FieldMapping {
    Title,
    DueDate { date_format: Option<String> },
    StartDate { date_format: Option<String> },
    Project,
    Tags { separator: String },
    Priority,
    Status,
    Notes,
    Uda { key: String, uda_type: UdaType },
}

/// Resolved mapping: column index → field mapping.
struct ResolvedMapping {
    columns: Vec<(usize, String, FieldMapping)>,
    defaults: HashMap<String, String>,
}

// ── Import report ─────────────────────────────────────────────────

/// Summary of a CSV import operation.
#[derive(Debug, Default)]
pub struct CsvImportReport {
    /// Number of tasks successfully imported.
    pub tasks_imported: usize,
    /// Number of rows skipped (e.g. empty title).
    pub rows_skipped: usize,
    /// Detected column mappings (header → tock field).
    pub columns_mapped: Vec<(String, String)>,
    /// Names of projects created during import.
    pub projects_created: Vec<String>,
    /// Number of UDA definitions registered.
    pub uda_definitions_created: usize,
    /// Warnings encountered during import.
    pub warnings: Vec<String>,
}

impl fmt::Display for CsvImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.columns_mapped.is_empty() {
            writeln!(f, "Column mapping:")?;
            for (header, field) in &self.columns_mapped {
                writeln!(f, "  {header} → {field}")?;
            }
        }
        writeln!(f, "Imported {} task(s)", self.tasks_imported)?;
        if self.rows_skipped > 0 {
            writeln!(f, "Skipped {} row(s) (empty title)", self.rows_skipped)?;
        }
        if !self.projects_created.is_empty() {
            writeln!(
                f,
                "Created {} project(s): {}",
                self.projects_created.len(),
                self.projects_created.join(", ")
            )?;
        }
        if self.uda_definitions_created > 0 {
            writeln!(
                f,
                "Registered {} UDA definition(s)",
                self.uda_definitions_created
            )?;
        }
        for warning in &self.warnings {
            writeln!(f, "  ⚠ {warning}")?;
        }
        Ok(())
    }
}

// ── Public API ────────────────────────────────────────────────────

/// Import tasks from CSV data with optional TOML mapping configuration.
///
/// The import runs inside a transaction for atomicity.
///
/// # Errors
///
/// Returns an error if the CSV is malformed, the TOML mapping is invalid,
/// or a storage operation fails. Row-level conversion issues are collected
/// as warnings rather than aborting.
pub fn import_csv(
    conn: &mut Connection,
    csv_data: &str,
    mapping_toml: Option<&str>,
) -> Result<CsvImportReport, tock_storage::Error> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(csv_data.as_bytes());

    let headers = reader
        .headers()
        .map_err(|e| csv_error(&e))?
        .iter()
        .map(strip_bom)
        .collect::<Vec<_>>();

    if headers.is_empty() {
        return Err(io_error("CSV file has no headers"));
    }

    let (mapping, mut report) = if let Some(toml_str) = mapping_toml {
        build_mapping_from_toml(&headers, toml_str)?
    } else {
        auto_detect_mapping(&headers)
    };

    // Verify we have a title column.
    if !mapping
        .columns
        .iter()
        .any(|(_, _, m)| matches!(m, FieldMapping::Title))
    {
        return Err(io_error(
            "no column maps to 'title' (need a description/title/task/name column)",
        ));
    }

    let tx = conn.transaction()?;
    import_rows(&tx, &mut reader, &mapping, &mut report)?;
    tx.commit()?;

    Ok(report)
}

// ── Auto-detection ────────────────────────────────────────────────

/// Normalize a header for matching: lowercase, strip non-alphanumeric.
fn normalize_header(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Strip UTF-8 BOM from the first header if present.
fn strip_bom(s: &str) -> String {
    s.strip_prefix('\u{feff}').unwrap_or(s).to_string()
}

/// Build mapping by matching headers against the alias dictionary.
fn auto_detect_mapping(headers: &[String]) -> (ResolvedMapping, CsvImportReport) {
    let mut report = CsvImportReport::default();
    let mut columns = Vec::new();

    for (idx, header) in headers.iter().enumerate() {
        let normalized = normalize_header(header);
        if normalized.is_empty() {
            continue;
        }

        if let Some(field_mapping) = match_alias(&normalized) {
            let label = field_mapping_label(&field_mapping);
            report
                .columns_mapped
                .push((header.clone(), label.to_string()));
            columns.push((idx, header.clone(), field_mapping));
        } else if RECOGNIZED_BUT_SKIPPED
            .iter()
            .any(|a| normalize_header(a) == normalized)
        {
            report.warnings.push(format!(
                "Column '{header}' (area) recognized but not imported"
            ));
        } else {
            let uda_key = sanitize_uda_key(&normalized);
            report
                .columns_mapped
                .push((header.clone(), format!("uda.{uda_key}")));
            columns.push((
                idx,
                header.clone(),
                FieldMapping::Uda {
                    key: uda_key,
                    uda_type: UdaType::String,
                },
            ));
        }
    }

    let mapping = ResolvedMapping {
        columns,
        defaults: HashMap::new(),
    };
    (mapping, report)
}

/// Match a normalized header against the built-in alias table.
fn match_alias(normalized: &str) -> Option<FieldMapping> {
    for (aliases, target) in ALIAS_TABLE {
        for alias in *aliases {
            if normalize_header(alias) == normalized {
                return Some(target_to_field_mapping(target));
            }
        }
    }
    None
}

/// Convert a target field name to its `FieldMapping` variant.
fn target_to_field_mapping(target: &str) -> FieldMapping {
    match target {
        "title" | "description" => FieldMapping::Title,
        "due_date" | "deadline" => FieldMapping::DueDate { date_format: None },
        "start_date" => FieldMapping::StartDate { date_format: None },
        "project" => FieldMapping::Project,
        "tags" => FieldMapping::Tags {
            separator: String::from(",;|"),
        },
        "priority" => FieldMapping::Priority,
        "status" => FieldMapping::Status,
        "notes" => FieldMapping::Notes,
        other => FieldMapping::Uda {
            key: sanitize_uda_key(other),
            uda_type: UdaType::String,
        },
    }
}

/// Sanitize a string into a valid UDA key (alphanumeric + underscores).
fn sanitize_uda_key(raw: &str) -> String {
    let key: String = raw
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    if key.is_empty() {
        String::from("unknown")
    } else {
        key
    }
}

/// Human-readable label for a field mapping.
const fn field_mapping_label(mapping: &FieldMapping) -> &str {
    match mapping {
        FieldMapping::Title => "title",
        FieldMapping::DueDate { .. } => "deadline",
        FieldMapping::StartDate { .. } => "start_date",
        FieldMapping::Project => "project",
        FieldMapping::Tags { .. } => "tags",
        FieldMapping::Priority => "priority",
        FieldMapping::Status => "status",
        FieldMapping::Notes => "notes",
        FieldMapping::Uda { .. } => "uda",
    }
}

// ── TOML mapping ──────────────────────────────────────────────────

/// Build mapping from a TOML configuration file.
fn build_mapping_from_toml(
    headers: &[String],
    toml_str: &str,
) -> Result<(ResolvedMapping, CsvImportReport), tock_storage::Error> {
    let doc: toml::Value =
        toml::from_str(toml_str).map_err(|e| io_error(&format!("invalid TOML mapping: {e}")))?;

    let mut report = CsvImportReport::default();
    let mut columns = Vec::new();

    // Build a case-insensitive header index.
    let header_index: HashMap<String, usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| (h.to_lowercase(), i))
        .collect();

    if let Some(cols) = doc.get("columns").and_then(|v| v.as_table()) {
        for (csv_col, mapping_value) in cols {
            let Some(&idx) = header_index.get(&csv_col.to_lowercase()) else {
                report.warnings.push(format!(
                    "TOML mapping references column '{csv_col}' not found in CSV headers"
                ));
                continue;
            };

            let field_mapping = parse_toml_column_mapping(mapping_value)?;
            let label = field_mapping_label(&field_mapping);
            report
                .columns_mapped
                .push((csv_col.clone(), label.to_string()));
            columns.push((idx, csv_col.clone(), field_mapping));
        }
    }

    let mut defaults = HashMap::new();
    if let Some(defs) = doc.get("defaults").and_then(|v| v.as_table()) {
        for (key, value) in defs {
            if let Some(s) = value.as_str() {
                defaults.insert(key.clone(), s.to_string());
            }
        }
    }

    let mapping = ResolvedMapping { columns, defaults };
    Ok((mapping, report))
}

/// Parse a single TOML column mapping entry.
fn parse_toml_column_mapping(value: &toml::Value) -> Result<FieldMapping, tock_storage::Error> {
    match value {
        toml::Value::String(s) => Ok(target_to_field_mapping(s)),
        toml::Value::Table(table) => {
            if let Some(uda_key) = table.get("uda").and_then(|v| v.as_str()) {
                let uda_type = table
                    .get("type")
                    .and_then(|v| v.as_str())
                    .and_then(UdaType::from_str_opt)
                    .unwrap_or(UdaType::String);
                return Ok(FieldMapping::Uda {
                    key: uda_key.to_string(),
                    uda_type,
                });
            }

            let field = table
                .get("field")
                .and_then(|v| v.as_str())
                .unwrap_or("title");

            let date_format = table
                .get("format")
                .and_then(|v| v.as_str())
                .map(String::from);
            let split = table
                .get("split")
                .and_then(|v| v.as_str())
                .map(String::from);

            match field {
                "due_date" => Ok(FieldMapping::DueDate { date_format }),
                "start_date" => Ok(FieldMapping::StartDate { date_format }),
                "tags" => Ok(FieldMapping::Tags {
                    separator: split.unwrap_or_else(|| String::from(",;|")),
                }),
                other => Ok(target_to_field_mapping(other)),
            }
        }
        _ => Err(io_error("invalid TOML column mapping value")),
    }
}

// ── Row import ────────────────────────────────────────────────────

/// Import all rows from the CSV reader.
fn import_rows(
    conn: &Connection,
    reader: &mut csv::Reader<&[u8]>,
    mapping: &ResolvedMapping,
    report: &mut CsvImportReport,
) -> Result<(), tock_storage::Error> {
    let existing_projects = tock_storage::repo::project_repo::list(conn, true)?;
    let mut project_cache: HashMap<String, uuid::Uuid> = existing_projects
        .into_iter()
        .map(|p| (p.name.clone(), p.id))
        .collect();

    let existing_udas = tock_storage::repo::uda_repo::list_definitions(conn)?;
    let mut known_uda_keys: HashSet<String> = existing_udas.into_iter().map(|d| d.key).collect();

    for (row_idx, result) in reader.records().enumerate() {
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                report
                    .warnings
                    .push(format!("Row {}: parse error: {e}", row_idx + 2));
                report.rows_skipped += 1;
                continue;
            }
        };

        match convert_row(
            &record,
            mapping,
            conn,
            &mut project_cache,
            &mut known_uda_keys,
            report,
            row_idx + 2,
        ) {
            Ok(Some(new_task)) => {
                tock_storage::repo::task_repo::insert(
                    conn,
                    &new_task,
                    &tock_core::domain::urgency::UrgencyConfig::default(),
                )?;
                report.tasks_imported += 1;
            }
            Ok(None) => {
                report.rows_skipped += 1;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

/// Convert a single CSV record to a `NewTask`. Returns `None` if the row
/// should be skipped (empty title).
#[allow(clippy::too_many_arguments)]
fn convert_row(
    record: &csv::StringRecord,
    mapping: &ResolvedMapping,
    conn: &Connection,
    project_cache: &mut HashMap<String, uuid::Uuid>,
    known_uda_keys: &mut HashSet<String>,
    report: &mut CsvImportReport,
    row_num: usize,
) -> Result<Option<NewTask>, tock_storage::Error> {
    let mut title = None;
    let mut notes = None;
    let mut deadline = None;
    let mut start_date = None;
    let mut project_name = None;
    let mut tags: Vec<String> = Vec::new();
    let mut priority = None;
    let mut status = None;
    let mut udas: BTreeMap<String, serde_json::Value> = BTreeMap::new();

    for (idx, _header, field_mapping) in &mapping.columns {
        let value = record.get(*idx).unwrap_or("").trim();
        if value.is_empty() {
            continue;
        }

        match field_mapping {
            FieldMapping::Title => title = Some(value.to_string()),
            FieldMapping::Notes => notes = Some(value.to_string()),
            FieldMapping::DueDate { date_format } => {
                deadline = parse_date_value(value, date_format.as_deref());
                if deadline.is_none() {
                    report
                        .warnings
                        .push(format!("Row {row_num}: invalid due date '{value}'"));
                }
            }
            FieldMapping::StartDate { date_format } => {
                start_date = parse_date_value(value, date_format.as_deref());
                if start_date.is_none() {
                    report
                        .warnings
                        .push(format!("Row {row_num}: invalid start date '{value}'"));
                }
            }
            FieldMapping::Project => project_name = Some(value.to_string()),
            FieldMapping::Tags { separator } => {
                let new_tags = split_tags(value, separator);
                tags.extend(new_tags);
            }
            FieldMapping::Priority => {
                priority = parse_priority_value(value);
                if priority.is_none() {
                    report
                        .warnings
                        .push(format!("Row {row_num}: unknown priority '{value}'"));
                }
            }
            FieldMapping::Status => {
                status = parse_status_value(value);
                if status.is_none() {
                    report.warnings.push(format!(
                        "Row {row_num}: unknown status '{value}', defaulting to Pending"
                    ));
                }
            }
            FieldMapping::Uda { key, uda_type } => {
                let json_value = uda_str_to_json(value, uda_type);
                udas.insert(key.clone(), json_value);
                register_uda_if_new(conn, key, uda_type, known_uda_keys, report)?;
            }
        }
    }

    // Apply defaults for missing fields.
    if title.is_none()
        && let Some(default_title) = mapping.defaults.get("title")
    {
        title = Some(default_title.clone());
    }
    if status.is_none()
        && let Some(default_status) = mapping.defaults.get("status")
    {
        status = parse_status_value(default_status);
    }

    // Skip rows with no title.
    let Some(title_str) = title else {
        report
            .warnings
            .push(format!("Row {row_num}: no title, skipping"));
        return Ok(None);
    };
    if title_str.is_empty() {
        report
            .warnings
            .push(format!("Row {row_num}: empty title, skipping"));
        return Ok(None);
    }

    let project_id = resolve_project(conn, project_name.as_deref(), project_cache, report)?;

    let new_task = NewTask {
        title: title_str,
        notes,
        status,
        project_id,
        deadline,
        start_date,
        priority,
        tags,
        udas: UdaValues(udas),
        ..NewTask::default()
    };

    Ok(Some(new_task))
}

// ── Field parsers ─────────────────────────────────────────────────

/// Parse a date value, trying ISO format first, then a custom format.
fn parse_date_value(raw: &str, custom_format: Option<&str>) -> Option<String> {
    // Try ISO YYYY-MM-DD first.
    if is_iso_date(raw) {
        return Some(raw.to_string());
    }

    // Try custom strftime-like format (common patterns).
    if let Some(fmt) = custom_format {
        return parse_date_with_format(raw, fmt);
    }

    // Try common US format MM/DD/YYYY.
    if let Some(date) = parse_date_with_format(raw, "%m/%d/%Y") {
        return Some(date);
    }

    // Try DD/MM/YYYY (European).
    if let Some(date) = parse_date_with_format(raw, "%d/%m/%Y") {
        return Some(date);
    }

    None
}

/// Check if a string is a valid ISO date (YYYY-MM-DD).
fn is_iso_date(raw: &str) -> bool {
    if raw.len() != 10 {
        return false;
    }
    let parts: Vec<&str> = raw.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit()))
}

/// Parse a date using a strftime-like format string.
/// Supports common patterns: `%Y`, `%m`, `%d`, `%y`.
fn parse_date_with_format(raw: &str, fmt: &str) -> Option<String> {
    let mut year: Option<u32> = None;
    let mut month: Option<u32> = None;
    let mut day: Option<u32> = None;

    let fmt_chars: Vec<char> = fmt.chars().collect();
    let raw_chars: Vec<char> = raw.chars().collect();
    let mut fi = 0;
    let mut ri = 0;

    while fi < fmt_chars.len() && ri < raw_chars.len() {
        if fmt_chars[fi] == '%' && fi + 1 < fmt_chars.len() {
            let spec = fmt_chars[fi + 1];
            fi += 2;
            match spec {
                'Y' => {
                    let (val, consumed) = consume_digits(&raw_chars, ri, 4)?;
                    year = Some(val);
                    ri += consumed;
                }
                'y' => {
                    let (val, consumed) = consume_digits(&raw_chars, ri, 2)?;
                    year = Some(if val < 70 { 2000 + val } else { 1900 + val });
                    ri += consumed;
                }
                'm' => {
                    let (val, consumed) = consume_digits(&raw_chars, ri, 2)?;
                    month = Some(val);
                    ri += consumed;
                }
                'd' => {
                    let (val, consumed) = consume_digits(&raw_chars, ri, 2)?;
                    day = Some(val);
                    ri += consumed;
                }
                _ => return None,
            }
        } else {
            // Literal character — must match.
            if raw_chars[ri] != fmt_chars[fi] {
                return None;
            }
            fi += 1;
            ri += 1;
        }
    }

    let y = year?;
    let m = month?;
    let d = day?;

    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y == 0 {
        return None;
    }

    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Consume up to `max_digits` digits from `chars` starting at `start`.
fn consume_digits(chars: &[char], start: usize, max_digits: usize) -> Option<(u32, usize)> {
    let mut end = start;
    while end < chars.len() && end - start < max_digits && chars[end].is_ascii_digit() {
        end += 1;
    }
    if end == start {
        return None;
    }
    let s: String = chars[start..end].iter().collect();
    let val = s.parse::<u32>().ok()?;
    Some((val, end - start))
}

/// Split tags by any character in the separator set.
fn split_tags(raw: &str, separators: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut current = String::new();

    for ch in raw.chars() {
        if separators.contains(ch) {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                tags.push(trimmed);
            }
            current.clear();
        } else {
            current.push(ch);
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        tags.push(trimmed);
    }

    tags
}

/// Parse a priority value from common representations.
fn parse_priority_value(raw: &str) -> Option<Priority> {
    let trimmed = raw.trim();
    match trimmed.to_uppercase().as_str() {
        "H" | "HIGH" | "!!!" | "1" => Some(Priority::High),
        "M" | "MEDIUM" | "MED" | "!!" | "2" => Some(Priority::Medium),
        "L" | "LOW" | "!" | "3" => Some(Priority::Low),
        _ => None,
    }
}

/// Parse a status value from common representations.
fn parse_status_value(raw: &str) -> Option<TaskStatus> {
    let lower = raw.trim().to_lowercase();
    match lower.as_str() {
        "pending" | "open" | "todo" | "to do" | "active" | "not started" => {
            Some(TaskStatus::Pending)
        }
        "done" | "completed" | "complete" | "finished" | "closed" | "yes" | "true" | "1" => {
            Some(TaskStatus::Done)
        }
        "cancelled" | "canceled" | "dropped" | "abandoned" => Some(TaskStatus::Cancelled),
        "someday" | "maybe" | "deferred" | "waiting" => Some(TaskStatus::Someday),
        "started" | "in progress" | "in-progress" | "wip" => Some(TaskStatus::Started),
        "inbox" | "new" => Some(TaskStatus::Inbox),
        _ => None,
    }
}

/// Convert a string UDA value to the appropriate JSON value based on type.
fn uda_str_to_json(raw: &str, uda_type: &UdaType) -> serde_json::Value {
    match uda_type {
        UdaType::Number => raw.parse::<f64>().map_or_else(
            |_| serde_json::Value::String(raw.to_string()),
            |n| serde_json::json!(n),
        ),
        UdaType::Boolean => {
            let lower = raw.to_lowercase();
            let b = matches!(lower.as_str(), "true" | "yes" | "1" | "y");
            serde_json::Value::Bool(b)
        }
        UdaType::Date | UdaType::String => serde_json::Value::String(raw.to_string()),
    }
}

// ── Shared helpers ────────────────────────────────────────────────

/// Register a UDA definition if it hasn't been seen before.
fn register_uda_if_new(
    conn: &Connection,
    key: &str,
    uda_type: &UdaType,
    known_uda_keys: &mut HashSet<String>,
    report: &mut CsvImportReport,
) -> Result<(), tock_storage::Error> {
    if known_uda_keys.insert(key.to_string()) {
        let def = UdaDefinition {
            key: key.to_string(),
            uda_type: uda_type.clone(),
            label: Some(key.to_string()),
            default: None,
        };
        tock_storage::repo::uda_repo::add_definition(conn, &def)?;
        report.uda_definitions_created += 1;
    }
    Ok(())
}

/// Find or create a project by name, caching results for dedup.
fn resolve_project(
    conn: &Connection,
    name: Option<&str>,
    cache: &mut HashMap<String, uuid::Uuid>,
    report: &mut CsvImportReport,
) -> Result<Option<uuid::Uuid>, tock_storage::Error> {
    let Some(raw_name) = name else {
        return Ok(None);
    };
    let trimmed = raw_name.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if let Some(&id) = cache.get(trimmed) {
        return Ok(Some(id));
    }
    let new_project = NewProject {
        name: trimmed.to_string(),
        notes: None,
        area_id: None,
        deadline: None,
    };
    let project = tock_storage::repo::project_repo::insert(conn, &new_project)?;
    cache.insert(trimmed.to_string(), project.id);
    report.projects_created.push(trimmed.to_string());
    Ok(Some(project.id))
}

/// Wrap a CSV error into `tock_storage::Error::Io`.
fn csv_error(e: &csv::Error) -> tock_storage::Error {
    tock_storage::Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        e.to_string(),
    ))
}

/// Create an `InvalidData` IO error.
fn io_error(msg: &str) -> tock_storage::Error {
    tock_storage::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, msg))
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;
    use tock_core::domain::task::TaskStatus;

    use super::*;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        tock_storage::migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    // ── Auto-detection ────────────────────────────────────────────

    #[test]
    fn normalizes_headers_case_insensitive() {
        assert_eq!(normalize_header("Due Date"), "duedate");
        assert_eq!(normalize_header("DUE_DATE"), "duedate");
        assert_eq!(normalize_header("due-date"), "duedate");
        assert_eq!(normalize_header("Due.Date"), "duedate");
    }

    #[test]
    fn auto_detects_standard_headers() {
        let headers: Vec<String> = vec![
            "Title".into(),
            "Due Date".into(),
            "Project".into(),
            "Tags".into(),
            "Priority".into(),
        ];
        let (mapping, report) = auto_detect_mapping(&headers);
        assert_eq!(mapping.columns.len(), 5);
        assert_eq!(report.columns_mapped.len(), 5);
        assert!(
            report
                .columns_mapped
                .iter()
                .any(|(h, f)| h == "Title" && f == "title")
        );
    }

    #[test]
    fn auto_detects_aliases() {
        let headers: Vec<String> = vec!["Task".into(), "Deadline".into(), "Labels".into()];
        let (mapping, _report) = auto_detect_mapping(&headers);
        assert_eq!(mapping.columns.len(), 3);
    }

    #[test]
    fn unmatched_columns_become_udas() {
        let headers: Vec<String> = vec!["Title".into(), "Effort".into(), "Custom Field".into()];
        let (mapping, report) = auto_detect_mapping(&headers);
        assert_eq!(mapping.columns.len(), 3);
        assert!(
            report
                .columns_mapped
                .iter()
                .any(|(_, f)| f.starts_with("uda."))
        );
    }

    #[test]
    fn warns_on_area_column() {
        let headers: Vec<String> = vec!["Title".into(), "Area".into()];
        let (_mapping, report) = auto_detect_mapping(&headers);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("area") && w.contains("not imported"))
        );
    }

    // ── Field parsers ─────────────────────────────────────────────

    #[test]
    fn parses_iso_date() {
        assert_eq!(
            parse_date_value("2026-03-15", None),
            Some("2026-03-15".into())
        );
    }

    #[test]
    fn parses_us_date_format() {
        assert_eq!(
            parse_date_value("03/15/2026", Some("%m/%d/%Y")),
            Some("2026-03-15".into())
        );
    }

    #[test]
    fn parses_european_date_format() {
        assert_eq!(
            parse_date_value("15/03/2026", Some("%d/%m/%Y")),
            Some("2026-03-15".into())
        );
    }

    #[test]
    fn rejects_invalid_date() {
        assert_eq!(parse_date_value("not-a-date", None), None);
    }

    #[test]
    fn splits_tags_by_multiple_separators() {
        let tags = split_tags("work, personal; urgent|low", ",;|");
        assert_eq!(tags, vec!["work", "personal", "urgent", "low"]);
    }

    #[test]
    fn parses_priority_variants() {
        assert_eq!(parse_priority_value("H"), Some(Priority::High));
        assert_eq!(parse_priority_value("high"), Some(Priority::High));
        assert_eq!(parse_priority_value("!!!"), Some(Priority::High));
        assert_eq!(parse_priority_value("1"), Some(Priority::High));
        assert_eq!(parse_priority_value("M"), Some(Priority::Medium));
        assert_eq!(parse_priority_value("2"), Some(Priority::Medium));
        assert_eq!(parse_priority_value("L"), Some(Priority::Low));
        assert_eq!(parse_priority_value("3"), Some(Priority::Low));
        assert_eq!(parse_priority_value("unknown"), None);
    }

    #[test]
    fn parses_status_variants() {
        assert_eq!(parse_status_value("pending"), Some(TaskStatus::Pending));
        assert_eq!(parse_status_value("todo"), Some(TaskStatus::Pending));
        assert_eq!(parse_status_value("done"), Some(TaskStatus::Done));
        assert_eq!(parse_status_value("completed"), Some(TaskStatus::Done));
        assert_eq!(parse_status_value("true"), Some(TaskStatus::Done));
        assert_eq!(parse_status_value("cancelled"), Some(TaskStatus::Cancelled));
        assert_eq!(parse_status_value("someday"), Some(TaskStatus::Someday));
        assert_eq!(parse_status_value("started"), Some(TaskStatus::Started));
        assert_eq!(parse_status_value("inbox"), Some(TaskStatus::Inbox));
    }

    // ── Full import (auto-detect) ─────────────────────────────────

    #[test]
    fn imports_basic_csv() {
        let mut conn = test_conn();
        let csv =
            "Title,Priority,Tags\nBuy groceries,M,\"shopping, personal\"\nClean house,L,home\n";
        let report = import_csv(&mut conn, csv, None).expect("import");
        assert_eq!(report.tasks_imported, 2);
        assert_eq!(report.rows_skipped, 0);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list");
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn imports_with_project_and_due_date() {
        let mut conn = test_conn();
        let csv =
            "Title,Project,Due Date\nFinish report,Work,2026-06-15\nBuy paint,Home,2026-07-01\n";
        let report = import_csv(&mut conn, csv, None).expect("import");
        assert_eq!(report.tasks_imported, 2);
        assert_eq!(report.projects_created.len(), 2);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list");
        assert!(tasks.iter().all(|t| t.project_id.is_some()));
        assert!(
            tasks
                .iter()
                .any(|t| t.deadline.as_deref() == Some("2026-06-15"))
        );
    }

    #[test]
    fn skips_rows_without_title() {
        let mut conn = test_conn();
        let csv = "Title,Priority\nBuy groceries,M\n,H\n";
        let report = import_csv(&mut conn, csv, None).expect("import");
        assert_eq!(report.tasks_imported, 1);
        assert_eq!(report.rows_skipped, 1);
    }

    #[test]
    fn imports_status_column() {
        let mut conn = test_conn();
        let csv = "Title,Status\nDone task,completed\nOpen task,pending\n";
        let report = import_csv(&mut conn, csv, None).expect("import");
        assert_eq!(report.tasks_imported, 2);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list");
        assert!(tasks.iter().any(|t| t.status == TaskStatus::Done));
        assert!(tasks.iter().any(|t| t.status == TaskStatus::Pending));
    }

    #[test]
    fn imports_notes_column() {
        let mut conn = test_conn();
        let csv = "Title,Notes\nTask with notes,Some detailed notes here\n";
        let report = import_csv(&mut conn, csv, None).expect("import");
        assert_eq!(report.tasks_imported, 1);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list");
        assert_eq!(tasks[0].notes.as_deref(), Some("Some detailed notes here"));
    }

    #[test]
    fn deduplicates_projects() {
        let mut conn = test_conn();
        let csv = "Title,Project\nTask A,Work\nTask B,Work\nTask C,Personal\n";
        let report = import_csv(&mut conn, csv, None).expect("import");
        assert_eq!(report.tasks_imported, 3);
        assert_eq!(report.projects_created.len(), 2);

        let projects = tock_storage::repo::project_repo::list(&conn, false).expect("list");
        assert_eq!(projects.len(), 2);
    }

    #[test]
    fn imports_uda_columns() {
        let mut conn = test_conn();
        let csv = "Title,Effort,Sprint\nTask A,high,42\n";
        let report = import_csv(&mut conn, csv, None).expect("import");
        assert_eq!(report.tasks_imported, 1);
        assert_eq!(report.uda_definitions_created, 2);

        let defs = tock_storage::repo::uda_repo::list_definitions(&conn).expect("list defs");
        assert!(defs.iter().any(|d| d.key == "effort"));
        assert!(defs.iter().any(|d| d.key == "sprint"));
    }

    #[test]
    fn handles_empty_csv() {
        let mut conn = test_conn();
        let csv = "Title,Priority\n";
        let report = import_csv(&mut conn, csv, None).expect("import");
        assert_eq!(report.tasks_imported, 0);
    }

    #[test]
    fn rejects_csv_without_title_column() {
        let mut conn = test_conn();
        let csv = "Priority,Status\nH,pending\n";
        let result = import_csv(&mut conn, csv, None);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_empty_csv() {
        let mut conn = test_conn();
        let result = import_csv(&mut conn, "", None);
        assert!(result.is_err());
    }

    // ── TOML mapping ──────────────────────────────────────────────

    #[test]
    fn imports_with_toml_mapping() {
        let mut conn = test_conn();
        let csv = "Task Name,Due,List\nBuy groceries,2026-06-15,Shopping\n";
        let toml = r#"
[columns]
"Task Name" = "description"
"Due"       = { field = "due_date" }
"List"      = "project"

[defaults]
status = "pending"
"#;
        let report = import_csv(&mut conn, csv, Some(toml)).expect("import");
        assert_eq!(report.tasks_imported, 1);
        assert_eq!(report.projects_created, vec!["Shopping"]);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list");
        assert_eq!(tasks[0].title, "Buy groceries");
        assert_eq!(tasks[0].deadline.as_deref(), Some("2026-06-15"));
        assert_eq!(tasks[0].status, TaskStatus::Pending);
    }

    #[test]
    fn toml_mapping_with_date_format() {
        let mut conn = test_conn();
        let csv = "Title,Due\nTask,03/15/2026\n";
        let toml = r#"
[columns]
"Title" = "description"
"Due"   = { field = "due_date", format = "%m/%d/%Y" }
"#;
        let report = import_csv(&mut conn, csv, Some(toml)).expect("import");
        assert_eq!(report.tasks_imported, 1);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list");
        assert_eq!(tasks[0].deadline.as_deref(), Some("2026-03-15"));
    }

    #[test]
    fn toml_mapping_with_tag_split() {
        let mut conn = test_conn();
        let csv = "Title,Tags\nTask,work:personal:urgent\n";
        let toml = r#"
[columns]
"Title" = "description"
"Tags"  = { field = "tags", split = ":" }
"#;
        let report = import_csv(&mut conn, csv, Some(toml)).expect("import");
        assert_eq!(report.tasks_imported, 1);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list");
        assert_eq!(tasks[0].tags.len(), 3);
        assert!(tasks[0].tags.contains(&"work".to_string()));
    }

    #[test]
    fn toml_mapping_with_typed_uda() {
        let mut conn = test_conn();
        let csv = "Title,Effort\nTask,42\n";
        let toml = r#"
[columns]
"Title"  = "description"
"Effort" = { uda = "effort", type = "number" }
"#;
        let report = import_csv(&mut conn, csv, Some(toml)).expect("import");
        assert_eq!(report.tasks_imported, 1);
        assert_eq!(report.uda_definitions_created, 1);

        let defs = tock_storage::repo::uda_repo::list_definitions(&conn).expect("list defs");
        let effort_def = defs
            .iter()
            .find(|d| d.key == "effort")
            .expect("find effort");
        assert_eq!(effort_def.uda_type, UdaType::Number);
    }

    #[test]
    fn toml_warns_on_missing_column() {
        let mut conn = test_conn();
        let csv = "Title\nTask\n";
        let toml = r#"
[columns]
"Title"     = "description"
"Nonexistent" = "project"
"#;
        let report = import_csv(&mut conn, csv, Some(toml)).expect("import");
        assert_eq!(report.tasks_imported, 1);
        assert!(report.warnings.iter().any(|w| w.contains("Nonexistent")));
    }

    #[test]
    fn handles_bom_in_first_header() {
        let mut conn = test_conn();
        let csv = "\u{feff}Title,Priority\nBuy groceries,H\n";
        let report = import_csv(&mut conn, csv, None).expect("import");
        assert_eq!(report.tasks_imported, 1);
    }

    #[test]
    fn imports_with_start_date() {
        let mut conn = test_conn();
        let csv = "Title,Start Date\nFuture task,2026-07-01\n";
        let report = import_csv(&mut conn, csv, None).expect("import");
        assert_eq!(report.tasks_imported, 1);

        let tasks = tock_storage::repo::task_repo::list(&conn, false).expect("list");
        assert_eq!(tasks[0].start_date.as_deref(), Some("2026-07-01"));
    }
}
