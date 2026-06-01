-- CalDAV link records: maps local entities to remote CalDAV resources.
-- See architecture §9.5 for the CalDAV integration design.

CREATE TABLE caldav_links (
    local_id       BLOB    NOT NULL,
    entity_type    TEXT    NOT NULL CHECK(entity_type IN ('task', 'time_block')),
    collection_url TEXT    NOT NULL,
    href           TEXT    NOT NULL,
    uid            TEXT    NOT NULL,
    etag           TEXT,
    last_pushed_at TEXT,
    last_pulled_at TEXT,
    PRIMARY KEY (local_id, collection_url)
);

CREATE INDEX caldav_links_href_idx ON caldav_links(href);
CREATE UNIQUE INDEX caldav_links_collection_uid_idx ON caldav_links(collection_url, uid, entity_type);

-- Per-collection sync state (sync tokens, ctags).
CREATE TABLE caldav_collections (
    url          TEXT PRIMARY KEY,
    display_name TEXT,
    sync_token   TEXT,
    ctag         TEXT,
    username     TEXT NOT NULL,
    last_sync_at TEXT
);
