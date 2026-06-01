//! Repository functions for `CalDAV` link records and collections.

use rusqlite::{Connection, OptionalExtension, Row, params};
use uuid::Uuid;

use super::{parse_uuid_blob, uuid_to_blob};
use crate::Error;

/// A `CalDAV` link record stored in the database.
#[derive(Clone, Debug)]
pub struct CalDavLinkRow {
    /// Local entity ID.
    pub local_id: Uuid,
    /// Entity type (`task` or `time_block`).
    pub entity_type: String,
    /// `CalDAV` collection URL.
    pub collection_url: String,
    /// Resource href on the server.
    pub href: String,
    /// `iCalendar` UID.
    pub uid: String,
    /// Last known `ETag`.
    pub etag: Option<String>,
    /// Last push timestamp (`ISO 8601`).
    pub last_pushed_at: Option<String>,
    /// Last pull timestamp (`ISO 8601`).
    pub last_pulled_at: Option<String>,
}

/// A `CalDAV` collection record.
#[derive(Clone, Debug)]
pub struct CalDavCollectionRow {
    /// Collection URL.
    pub url: String,
    /// Display name.
    pub display_name: Option<String>,
    /// Sync token for incremental sync.
    pub sync_token: Option<String>,
    /// `CTag` (collection-level change indicator).
    pub ctag: Option<String>,
    /// Username for authentication.
    pub username: String,
    /// Last sync timestamp (`ISO 8601`).
    pub last_sync_at: Option<String>,
}

fn row_to_link(row: &Row<'_>) -> Result<CalDavLinkRow, rusqlite::Error> {
    let id_blob: Vec<u8> = row.get(0)?;
    let local_id =
        parse_uuid_blob(&id_blob).map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
    Ok(CalDavLinkRow {
        local_id,
        entity_type: row.get(1)?,
        collection_url: row.get(2)?,
        href: row.get(3)?,
        uid: row.get(4)?,
        etag: row.get(5)?,
        last_pushed_at: row.get(6)?,
        last_pulled_at: row.get(7)?,
    })
}

fn row_to_collection(row: &Row<'_>) -> Result<CalDavCollectionRow, rusqlite::Error> {
    Ok(CalDavCollectionRow {
        url: row.get(0)?,
        display_name: row.get(1)?,
        sync_token: row.get(2)?,
        ctag: row.get(3)?,
        username: row.get(4)?,
        last_sync_at: row.get(5)?,
    })
}

// ── CalDAV links ──────────────────────────────────────────────────

/// Insert or update a `CalDAV` link record.
///
/// # Errors
/// Returns [`Error::Sqlite`] on database failures.
pub fn upsert_link(conn: &Connection, link: &CalDavLinkRow) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO caldav_links (local_id, entity_type, collection_url, href, uid, etag, last_pushed_at, last_pulled_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(local_id, collection_url) DO UPDATE SET
             href = excluded.href,
             uid = excluded.uid,
             etag = excluded.etag,
             last_pushed_at = excluded.last_pushed_at,
             last_pulled_at = excluded.last_pulled_at",
        params![
            uuid_to_blob(link.local_id),
            link.entity_type,
            link.collection_url,
            link.href,
            link.uid,
            link.etag,
            link.last_pushed_at,
            link.last_pulled_at,
        ],
    )?;
    Ok(())
}

/// Get a link by local ID and collection URL.
///
/// # Errors
/// Returns [`Error::Sqlite`] on database failures.
pub fn get_link(
    conn: &Connection,
    local_id: Uuid,
    collection_url: &str,
) -> Result<Option<CalDavLinkRow>, Error> {
    conn.query_row(
        "SELECT local_id, entity_type, collection_url, href, uid, etag, last_pushed_at, last_pulled_at
         FROM caldav_links WHERE local_id = ?1 AND collection_url = ?2",
        params![uuid_to_blob(local_id), collection_url],
        row_to_link,
    )
    .optional()
    .map_err(Error::from)
}

/// Get a link by remote href.
///
/// # Errors
/// Returns [`Error::Sqlite`] on database failures.
pub fn get_link_by_href(conn: &Connection, href: &str) -> Result<Option<CalDavLinkRow>, Error> {
    conn.query_row(
        "SELECT local_id, entity_type, collection_url, href, uid, etag, last_pushed_at, last_pulled_at
         FROM caldav_links WHERE href = ?1",
        params![href],
        row_to_link,
    )
    .optional()
    .map_err(Error::from)
}

/// List all links for a given collection.
///
/// # Errors
/// Returns [`Error::Sqlite`] on database failures.
pub fn list_links(conn: &Connection, collection_url: &str) -> Result<Vec<CalDavLinkRow>, Error> {
    let mut stmt = conn.prepare(
        "SELECT local_id, entity_type, collection_url, href, uid, etag, last_pushed_at, last_pulled_at
         FROM caldav_links WHERE collection_url = ?1",
    )?;
    let rows = stmt.query_map(params![collection_url], row_to_link)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Delete a link by local ID and collection URL.
///
/// # Errors
/// Returns [`Error::Sqlite`] on database failures.
pub fn delete_link(conn: &Connection, local_id: Uuid, collection_url: &str) -> Result<(), Error> {
    conn.execute(
        "DELETE FROM caldav_links WHERE local_id = ?1 AND collection_url = ?2",
        params![uuid_to_blob(local_id), collection_url],
    )?;
    Ok(())
}

/// Delete a link by remote href.
///
/// # Errors
/// Returns [`Error::Sqlite`] on database failures.
pub fn delete_link_by_href(conn: &Connection, href: &str) -> Result<(), Error> {
    conn.execute("DELETE FROM caldav_links WHERE href = ?1", params![href])?;
    Ok(())
}

// ── CalDAV collections ────────────────────────────────────────────

/// Insert or update a collection record.
///
/// # Errors
/// Returns [`Error::Sqlite`] on database failures.
pub fn upsert_collection(conn: &Connection, col: &CalDavCollectionRow) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO caldav_collections (url, display_name, sync_token, ctag, username, last_sync_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(url) DO UPDATE SET
             display_name = excluded.display_name,
             sync_token = excluded.sync_token,
             ctag = excluded.ctag,
             username = excluded.username,
             last_sync_at = excluded.last_sync_at",
        params![
            col.url,
            col.display_name,
            col.sync_token,
            col.ctag,
            col.username,
            col.last_sync_at,
        ],
    )?;
    Ok(())
}

/// Get a collection by URL.
///
/// # Errors
/// Returns [`Error::Sqlite`] on database failures.
pub fn get_collection(conn: &Connection, url: &str) -> Result<Option<CalDavCollectionRow>, Error> {
    conn.query_row(
        "SELECT url, display_name, sync_token, ctag, username, last_sync_at
         FROM caldav_collections WHERE url = ?1",
        params![url],
        row_to_collection,
    )
    .optional()
    .map_err(Error::from)
}

/// List all configured collections.
///
/// # Errors
/// Returns [`Error::Sqlite`] on database failures.
pub fn list_collections(conn: &Connection) -> Result<Vec<CalDavCollectionRow>, Error> {
    let mut stmt = conn.prepare(
        "SELECT url, display_name, sync_token, ctag, username, last_sync_at
         FROM caldav_collections",
    )?;
    let rows = stmt.query_map([], row_to_collection)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Delete a collection and all its links.
///
/// # Errors
/// Returns [`Error::Sqlite`] on database failures.
pub fn delete_collection(conn: &Connection, url: &str) -> Result<(), Error> {
    conn.execute(
        "DELETE FROM caldav_links WHERE collection_url = ?1",
        params![url],
    )?;
    conn.execute(
        "DELETE FROM caldav_collections WHERE url = ?1",
        params![url],
    )?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::vault;
    use tempfile::TempDir;

    fn setup() -> (TempDir, vault::OpenVault) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let v = vault::init(&path, b"password123").expect("vault init");
        (dir, v)
    }

    #[test]
    fn upsert_and_get_link() {
        let (_dir, vault) = setup();
        let conn = vault.connection();
        let id = Uuid::now_v7();
        let link = CalDavLinkRow {
            local_id: id,
            entity_type: "task".into(),
            collection_url: "https://cal.example.com/tasks/".into(),
            href: "/cal/tasks/test.ics".into(),
            uid: "test-uid".into(),
            etag: Some("\"etag1\"".into()),
            last_pushed_at: Some("2026-06-01T00:00:00Z".into()),
            last_pulled_at: None,
        };
        upsert_link(conn, &link).expect("upsert");

        let got = get_link(conn, id, "https://cal.example.com/tasks/")
            .expect("get")
            .expect("found");
        assert_eq!(got.uid, "test-uid");
        assert_eq!(got.etag.as_deref(), Some("\"etag1\""));
    }

    #[test]
    fn upsert_updates_existing() {
        let (_dir, vault) = setup();
        let conn = vault.connection();
        let id = Uuid::now_v7();
        let mut link = CalDavLinkRow {
            local_id: id,
            entity_type: "task".into(),
            collection_url: "https://cal.example.com/tasks/".into(),
            href: "/cal/tasks/test.ics".into(),
            uid: "test-uid".into(),
            etag: Some("\"etag1\"".into()),
            last_pushed_at: None,
            last_pulled_at: None,
        };
        upsert_link(conn, &link).expect("upsert 1");

        link.etag = Some("\"etag2\"".into());
        upsert_link(conn, &link).expect("upsert 2");

        let got = get_link(conn, id, "https://cal.example.com/tasks/")
            .expect("get")
            .expect("found");
        assert_eq!(got.etag.as_deref(), Some("\"etag2\""));
    }

    #[test]
    fn get_link_by_href_works() {
        let (_dir, vault) = setup();
        let conn = vault.connection();
        let link = CalDavLinkRow {
            local_id: Uuid::now_v7(),
            entity_type: "time_block".into(),
            collection_url: "https://cal.example.com/events/".into(),
            href: "/cal/events/block.ics".into(),
            uid: "block-uid".into(),
            etag: None,
            last_pushed_at: None,
            last_pulled_at: None,
        };
        upsert_link(conn, &link).expect("upsert");

        let got = get_link_by_href(conn, "/cal/events/block.ics")
            .expect("get")
            .expect("found");
        assert_eq!(got.uid, "block-uid");
        assert_eq!(got.entity_type, "time_block");
    }

    #[test]
    fn list_links_filters_by_collection() {
        let (_dir, vault) = setup();
        let conn = vault.connection();
        let url1 = "https://cal.example.com/tasks/";
        let url2 = "https://cal.example.com/events/";
        upsert_link(
            conn,
            &CalDavLinkRow {
                local_id: Uuid::now_v7(),
                entity_type: "task".into(),
                collection_url: url1.into(),
                href: "/cal/tasks/1.ics".into(),
                uid: "uid1".into(),
                etag: None,
                last_pushed_at: None,
                last_pulled_at: None,
            },
        )
        .expect("upsert 1");
        upsert_link(
            conn,
            &CalDavLinkRow {
                local_id: Uuid::now_v7(),
                entity_type: "time_block".into(),
                collection_url: url2.into(),
                href: "/cal/events/2.ics".into(),
                uid: "uid2".into(),
                etag: None,
                last_pushed_at: None,
                last_pulled_at: None,
            },
        )
        .expect("upsert 2");

        let tasks = list_links(conn, url1).expect("list");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].uid, "uid1");
    }

    #[test]
    fn delete_link_removes() {
        let (_dir, vault) = setup();
        let conn = vault.connection();
        let id = Uuid::now_v7();
        let url = "https://cal.example.com/tasks/";
        upsert_link(
            conn,
            &CalDavLinkRow {
                local_id: id,
                entity_type: "task".into(),
                collection_url: url.into(),
                href: "/cal/tasks/del.ics".into(),
                uid: "del-uid".into(),
                etag: None,
                last_pushed_at: None,
                last_pulled_at: None,
            },
        )
        .expect("upsert");

        delete_link(conn, id, url).expect("delete");
        let got = get_link(conn, id, url).expect("get");
        assert!(got.is_none());
    }

    #[test]
    fn collection_crud() {
        let (_dir, vault) = setup();
        let conn = vault.connection();
        let col = CalDavCollectionRow {
            url: "https://cal.example.com/tasks/".into(),
            display_name: Some("Work Tasks".into()),
            sync_token: Some("tok1".into()),
            ctag: None,
            username: "user".into(),
            last_sync_at: None,
        };
        upsert_collection(conn, &col).expect("upsert");

        let got = get_collection(conn, &col.url).expect("get").expect("found");
        assert_eq!(got.display_name.as_deref(), Some("Work Tasks"));
        assert_eq!(got.sync_token.as_deref(), Some("tok1"));

        let all = list_collections(conn).expect("list");
        assert_eq!(all.len(), 1);

        delete_collection(conn, &col.url).expect("delete");
        assert!(get_collection(conn, &col.url).expect("get").is_none());
    }
}
