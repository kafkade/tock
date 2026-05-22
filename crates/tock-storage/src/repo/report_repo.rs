//! Repository functions for saved reports.

use rusqlite::{Connection, Row, params};
use time::OffsetDateTime;
use tock_core::domain::report::{NewReport, Report};
use uuid::Uuid;

use super::{format_timestamp, invalid_encoding, parse_timestamp, parse_uuid_blob, uuid_to_blob};
use crate::Error;

const SELECT_REPORT_SQL: &str =
    "SELECT id, name, query, sort, columns, created_at, modified_at FROM saved_reports";

/// Insert a new saved report and return the stored row.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored UUID, timestamp, or JSON data is invalid.
pub fn insert(conn: &Connection, new: &NewReport) -> Result<Report, Error> {
    let id = Uuid::now_v7();
    let created_at = OffsetDateTime::now_utc();
    let created_at_text = format_timestamp(created_at)?;
    let columns = serde_json::to_string(&new.columns).map_err(|_| invalid_encoding())?;

    conn.execute(
        "INSERT INTO saved_reports (id, name, query, sort, columns, created_at, modified_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            uuid_to_blob(id),
            new.name,
            new.query,
            new.sort,
            columns,
            created_at_text,
            created_at_text,
        ],
    )?;

    get_by_name(conn, &new.name)?.ok_or(Error::NotFound)
}

/// Fetch a saved report by name.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored UUID, timestamp, or JSON data is invalid.
pub fn get_by_name(conn: &Connection, name: &str) -> Result<Option<Report>, Error> {
    let sql = format!("{SELECT_REPORT_SQL} WHERE name = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![name])?;
    if let Some(row) = rows.next()? {
        Ok(Some(read_report_row(row)?))
    } else {
        Ok(None)
    }
}

/// List all saved reports ordered by name.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::Core`] if stored UUID, timestamp, or JSON data is invalid.
pub fn list(conn: &Connection) -> Result<Vec<Report>, Error> {
    let sql = format!("{SELECT_REPORT_SQL} ORDER BY name ASC");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut reports = Vec::new();
    while let Some(row) = rows.next()? {
        reports.push(read_report_row(row)?);
    }
    Ok(reports)
}

/// Delete a saved report by name.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures.
pub fn delete(conn: &Connection, name: &str) -> Result<(), Error> {
    conn.execute("DELETE FROM saved_reports WHERE name = ?1", params![name])?;
    Ok(())
}

fn read_report_row(row: &Row<'_>) -> Result<Report, Error> {
    let columns_raw: String = row.get("columns")?;
    let columns = serde_json::from_str(&columns_raw).map_err(|_| invalid_encoding())?;
    Ok(Report {
        id: parse_uuid_blob(&row.get::<_, Vec<u8>>("id")?)?,
        name: row.get("name")?,
        query: row.get("query")?,
        sort: row.get("sort")?,
        columns,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
        modified_at: parse_timestamp(&row.get::<_, String>("modified_at")?)?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;
    use tock_core::domain::report::NewReport;

    use super::{delete, get_by_name, insert, list};
    use crate::migrations;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    #[test]
    fn inserts_lists_and_deletes_reports() {
        let conn = test_conn();
        let new = NewReport {
            name: String::from("work"),
            query: String::from("tag:work status:pending"),
            sort: Some(String::from("deadline")),
            columns: vec![String::from("sid"), String::from("title")],
        };

        let inserted = insert(&conn, &new).expect("insert report");
        assert_eq!(inserted.name, "work");
        assert_eq!(inserted.query, "tag:work status:pending");
        assert_eq!(inserted.sort.as_deref(), Some("deadline"));
        assert_eq!(
            inserted.columns,
            vec![String::from("sid"), String::from("title")]
        );

        let fetched = get_by_name(&conn, "work")
            .expect("fetch report")
            .expect("report exists");
        assert_eq!(fetched.id, inserted.id);

        let reports = list(&conn).expect("list reports");
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].name, "work");

        delete(&conn, "work").expect("delete report");
        assert!(
            get_by_name(&conn, "work")
                .expect("fetch after delete")
                .is_none()
        );
    }
}
