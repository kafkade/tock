CREATE TABLE saved_reports (
    id          BLOB PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    query       TEXT NOT NULL,
    sort        TEXT,
    columns     TEXT NOT NULL DEFAULT '[]',
    created_at  TEXT NOT NULL,
    modified_at TEXT NOT NULL
);
