//! Repository functions for projects.

use rusqlite::{Connection, Row, params};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    format_timestamp, parse_optional_timestamp, parse_optional_uuid_blob, parse_timestamp,
    parse_u32, parse_uuid_blob, uuid_to_blob,
};
use crate::Error;
use tock_core::domain::project::{NewProject, Project, ProjectStatus};
use tock_core::domain::sid::SidKind;

use crate::repo::sid_repo;

const SELECT_PROJECT_SQL: &str = "SELECT id, sid, area_id, name, notes, deadline, status, sort_order, created_at, modified_at, archived_at FROM projects";

/// Insert a new project row and return it.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn insert(conn: &Connection, input: &NewProject) -> Result<Project, Error> {
    let id = Uuid::now_v7();
    let sid = sid_repo::next_sid(conn, SidKind::Project)?;
    let created_at = OffsetDateTime::now_utc();
    let created_at_text = format_timestamp(created_at)?;

    conn.execute(
        "INSERT INTO projects (
             id, sid, area_id, name, notes, deadline, status, sort_order,
             done_at, cancelled_at, archived_at, created_at, modified_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, NULL, NULL, NULL, ?8, ?9)",
        params![
            uuid_to_blob(id),
            i64::from(sid),
            input.area_id.map(uuid_to_blob),
            input.name,
            input.notes,
            input.deadline,
            ProjectStatus::Active.as_str(),
            created_at_text,
            created_at_text,
        ],
    )?;

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

/// Fetch a project by SID.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn get_by_sid(conn: &Connection, sid: u32) -> Result<Option<Project>, Error> {
    let sql = format!("{SELECT_PROJECT_SQL} WHERE sid = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![i64::from(sid)])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(read_project_row(row)?));
    }
    Ok(None)
}

/// List projects.
///
/// Archived projects are excluded unless `include_archived` is `true`.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and
/// [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn list(conn: &Connection, include_archived: bool) -> Result<Vec<Project>, Error> {
    let sql = if include_archived {
        format!("{SELECT_PROJECT_SQL} ORDER BY sort_order ASC, sid ASC")
    } else {
        format!("{SELECT_PROJECT_SQL} WHERE archived_at IS NULL ORDER BY sort_order ASC, sid ASC")
    };

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![])?;
    let mut projects = Vec::new();
    while let Some(row) = rows.next()? {
        projects.push(read_project_row(row)?);
    }
    Ok(projects)
}

/// Archive a project by SID.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures,
/// [`crate::Error::NotFound`] if the project does not exist, and
/// [`crate::Error::Core`] if the timestamp cannot be formatted.
pub fn archive(conn: &Connection, sid: u32) -> Result<(), Error> {
    let now_text = format_timestamp(OffsetDateTime::now_utc())?;
    let rows_affected = conn.execute(
        "UPDATE projects SET archived_at = ?1, modified_at = ?2 WHERE sid = ?3",
        params![now_text, now_text, i64::from(sid)],
    )?;

    if rows_affected == 0 {
        return Err(Error::NotFound);
    }

    Ok(())
}

fn read_project_row(row: &Row<'_>) -> Result<Project, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    let sid_value: i64 = row.get("sid")?;
    let status_raw: String = row.get("status")?;

    Ok(Project {
        id: parse_uuid_blob(&id_bytes)?,
        sid: parse_u32(sid_value)?,
        name: row.get("name")?,
        notes: row.get("notes")?,
        status: parse_project_status(&status_raw)?,
        area_id: parse_optional_uuid_blob(row.get::<_, Option<Vec<u8>>>("area_id")?.as_deref())?,
        deadline: row.get("deadline")?,
        sort_order: row.get("sort_order")?,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
        modified_at: parse_timestamp(&row.get::<_, String>("modified_at")?)?,
        archived_at: parse_optional_timestamp(
            row.get::<_, Option<String>>("archived_at")?.as_deref(),
        )?,
    })
}

fn parse_project_status(raw: &str) -> Result<ProjectStatus, Error> {
    ProjectStatus::from_str_opt(raw).ok_or_else(super::invalid_encoding)
}
