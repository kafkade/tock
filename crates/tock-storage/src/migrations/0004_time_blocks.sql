CREATE TABLE time_blocks (
    id           BLOB PRIMARY KEY,
    sid          INTEGER NOT NULL UNIQUE,
    title        TEXT NOT NULL,
    start_ts     TEXT NOT NULL,
    end_ts       TEXT,
    project_id   BLOB REFERENCES projects(id) ON DELETE SET NULL,
    task_id      BLOB REFERENCES tasks(id)    ON DELETE SET NULL,
    notes        TEXT,
    source       TEXT NOT NULL CHECK (source IN ('manual','timer','pomodoro','imported')),
    billable     INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL,
    modified_at  TEXT NOT NULL
);
CREATE INDEX time_blocks_start_idx    ON time_blocks(start_ts);
CREATE INDEX time_blocks_open_idx     ON time_blocks(end_ts) WHERE end_ts IS NULL;
CREATE INDEX time_blocks_task_idx     ON time_blocks(task_id);
CREATE INDEX time_blocks_project_idx  ON time_blocks(project_id);
