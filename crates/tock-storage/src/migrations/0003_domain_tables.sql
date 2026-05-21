-- Phase 1 domain tables: areas, projects, headings, tasks, tags, SID counters.
-- Per architecture §3.2 schema.

-- ───────────── areas / projects ─────────────
CREATE TABLE areas (
    id           BLOB PRIMARY KEY,
    name         TEXT NOT NULL,
    color        TEXT,
    sort_order   INTEGER NOT NULL DEFAULT 0,
    archived_at  TEXT,
    created_at   TEXT NOT NULL,
    modified_at  TEXT NOT NULL
);

CREATE TABLE projects (
    id            BLOB PRIMARY KEY,
    sid           INTEGER NOT NULL UNIQUE,
    area_id       BLOB REFERENCES areas(id) ON DELETE SET NULL,
    name          TEXT NOT NULL,
    notes         TEXT,
    deadline      TEXT,
    status        TEXT NOT NULL DEFAULT 'active' CHECK (status IN
                  ('active','someday','paused','done','cancelled')),
    sort_order    INTEGER NOT NULL DEFAULT 0,
    done_at       TEXT,
    cancelled_at  TEXT,
    archived_at   TEXT,
    created_at    TEXT NOT NULL,
    modified_at   TEXT NOT NULL
);
CREATE INDEX projects_status_idx ON projects(status) WHERE archived_at IS NULL;
CREATE INDEX projects_area_idx   ON projects(area_id);

CREATE TABLE headings (
    id          BLOB PRIMARY KEY,
    project_id  BLOB NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    modified_at TEXT NOT NULL
);
CREATE INDEX headings_project_idx ON headings(project_id);

-- ───────────── tasks ─────────────
CREATE TABLE tasks (
    id              BLOB PRIMARY KEY,
    sid             INTEGER NOT NULL UNIQUE,
    title           TEXT NOT NULL,
    notes           TEXT,
    status          TEXT NOT NULL CHECK (status IN
                    ('inbox','pending','started','done','cancelled','someday')),
    area_id         BLOB REFERENCES areas(id)    ON DELETE SET NULL,
    project_id      BLOB REFERENCES projects(id) ON DELETE SET NULL,
    heading_id      BLOB REFERENCES headings(id) ON DELETE SET NULL,
    start_date      TEXT,
    deadline        TEXT,
    scheduled_for   TEXT,
    evening         INTEGER NOT NULL DEFAULT 0,
    priority        TEXT CHECK (priority IS NULL OR priority IN ('L','M','H')),
    udas            TEXT NOT NULL DEFAULT '{}',
    urgency_cache   REAL NOT NULL DEFAULT 0.0,
    created_at      TEXT NOT NULL,
    modified_at     TEXT NOT NULL,
    done_at         TEXT,
    cancelled_at    TEXT,
    deleted_at      TEXT
);
CREATE INDEX tasks_status_idx     ON tasks(status) WHERE deleted_at IS NULL;
CREATE INDEX tasks_project_idx    ON tasks(project_id);
CREATE INDEX tasks_deadline_idx   ON tasks(deadline) WHERE deadline IS NOT NULL;
CREATE INDEX tasks_urgency_idx    ON tasks(urgency_cache DESC) WHERE status IN ('pending','started');
CREATE INDEX tasks_modified_idx   ON tasks(modified_at);

-- ───────────── tags (N:N) ─────────────
CREATE TABLE tags (
    id    BLOB PRIMARY KEY,
    name  TEXT NOT NULL UNIQUE,
    color TEXT
);

CREATE TABLE entity_tags (
    entity_id   BLOB NOT NULL,
    entity_kind TEXT NOT NULL CHECK (entity_kind IN ('task','project','habit','block')),
    tag_id      BLOB NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (entity_id, entity_kind, tag_id)
);
CREATE INDEX entity_tags_tag_idx ON entity_tags(tag_id);

-- ───────────── SID allocator ─────────────
CREATE TABLE sid_counters (
    kind TEXT PRIMARY KEY,
    next INTEGER NOT NULL DEFAULT 1
);
INSERT INTO sid_counters (kind, next) VALUES ('task', 1);
INSERT INTO sid_counters (kind, next) VALUES ('project', 1);
INSERT INTO sid_counters (kind, next) VALUES ('habit', 1);
INSERT INTO sid_counters (kind, next) VALUES ('block', 1);
INSERT INTO sid_counters (kind, next) VALUES ('focus', 1);

-- ───────────── annotations ─────────────
CREATE TABLE annotations (
    id          BLOB PRIMARY KEY,
    entity_id   BLOB NOT NULL,
    entity_kind TEXT NOT NULL CHECK (entity_kind IN ('task','project','habit','block')),
    body        TEXT NOT NULL,
    created_at  TEXT NOT NULL
);
CREATE INDEX annotations_entity_idx ON annotations(entity_id, entity_kind);
