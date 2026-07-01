-- Checklist items: ordered sub-task checkboxes belonging to a task.
-- Per architecture §2.1 — title + done state only, no scheduling/tags/nesting.

CREATE TABLE checklist_items (
    id          BLOB PRIMARY KEY,
    task_id     BLOB NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    position    INTEGER NOT NULL DEFAULT 0,
    done_at     TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX checklist_items_task_idx ON checklist_items(task_id, position);
