CREATE TABLE habits (
    id             BLOB PRIMARY KEY,
    sid            INTEGER NOT NULL UNIQUE,
    title          TEXT NOT NULL,
    identity       TEXT,
    cue            TEXT,
    craving        TEXT,
    response       TEXT,
    reward         TEXT,
    direction      TEXT NOT NULL CHECK (direction IN ('build','break')),
    cadence        TEXT NOT NULL,
    minimum        TEXT NOT NULL,
    stack_after    BLOB REFERENCES habits(id) ON DELETE SET NULL,
    stack_delay_s  INTEGER NOT NULL DEFAULT 0,
    area_id        BLOB REFERENCES areas(id) ON DELETE SET NULL,
    project_id     BLOB REFERENCES projects(id) ON DELETE SET NULL,
    level          INTEGER NOT NULL DEFAULT 1,
    xp             INTEGER NOT NULL DEFAULT 0,
    streak_current INTEGER NOT NULL DEFAULT 0,
    streak_best    INTEGER NOT NULL DEFAULT 0,
    reminders      TEXT NOT NULL DEFAULT '[]',
    accountability TEXT,
    source_task_id BLOB REFERENCES tasks(id) ON DELETE SET NULL,
    created_at     TEXT NOT NULL,
    modified_at    TEXT NOT NULL,
    archived_at    TEXT
);
CREATE INDEX habits_active_idx ON habits(archived_at) WHERE archived_at IS NULL;

CREATE TABLE habit_entries (
    id          BLOB PRIMARY KEY,
    habit_id    BLOB NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
    occurred_at TEXT NOT NULL,
    amount      TEXT NOT NULL,
    notes       TEXT,
    slip        INTEGER NOT NULL DEFAULT 0,
    source      TEXT NOT NULL CHECK (source IN ('cli','timer','apple','sync','import')),
    created_at  TEXT NOT NULL
);
CREATE INDEX habit_entries_habit_idx ON habit_entries(habit_id, occurred_at DESC);

CREATE TABLE habit_skips (
    id         BLOB PRIMARY KEY,
    habit_id   BLOB NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
    date       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('skip','freeze')),
    reason     TEXT,
    created_at TEXT NOT NULL,
    UNIQUE (habit_id, date)
);
