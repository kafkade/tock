-- Undo / redo journal (issue #150).
--
-- Domain repositories write directly to SQLite and do not emit
-- per-operation events (the event log is only synthesized at sync
-- time). To make mutating CLI commands reversible we record a
-- row-level before/after diff around each command in this table.
--
-- The journal is device-local session state: it is deliberately NOT
-- part of the sync registry, so it never leaves the device. Each entry
-- stores a serialized change set (inserts / updates / deletes of domain
-- rows) plus a human label used for `Undid: <label>` feedback.
--
-- Undo/redo is a linear stack: `undone = 0` entries form the undo stack
-- (newest first) and `undone = 1` entries form the redo stack. Recording
-- a new entry clears the redo stack.
CREATE TABLE undo_log (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    label       TEXT NOT NULL,          -- human action label (e.g. "done 42")
    changes     BLOB NOT NULL,          -- serialized JSON change set
    undone      INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL
);
CREATE INDEX undo_log_undone_idx ON undo_log (undone, seq);
