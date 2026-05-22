CREATE TABLE contexts (
    name   TEXT PRIMARY KEY,
    filter TEXT NOT NULL
);

CREATE TABLE task_dependencies (
    task_id        BLOB NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    depends_on_id  BLOB NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, depends_on_id),
    CHECK (task_id != depends_on_id)
);
CREATE INDEX task_dependencies_depends_on_idx ON task_dependencies(depends_on_id);

ALTER TABLE tasks ADD COLUMN recurrence TEXT;
ALTER TABLE tasks ADD COLUMN parent_id BLOB REFERENCES tasks(id) ON DELETE SET NULL;
CREATE INDEX tasks_parent_idx ON tasks(parent_id);
