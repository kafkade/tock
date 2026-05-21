CREATE TABLE focus_sessions (
    id               BLOB PRIMARY KEY,
    sid              INTEGER NOT NULL UNIQUE,
    started_at       TEXT NOT NULL,
    ended_at         TEXT,
    task_id          BLOB REFERENCES tasks(id) ON DELETE SET NULL,
    project_id       BLOB REFERENCES projects(id) ON DELETE SET NULL,
    planned_cycles   INTEGER NOT NULL,
    completed_cycles INTEGER NOT NULL DEFAULT 0,
    state            TEXT NOT NULL CHECK (state IN
                     ('working','short_break','long_break','paused','aborted','completed')),
    config_snapshot  TEXT NOT NULL,
    created_at       TEXT NOT NULL,
    modified_at      TEXT NOT NULL
);
CREATE INDEX focus_started_idx ON focus_sessions(started_at);
