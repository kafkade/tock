-- Application-local scalar state (issue #155).
--
-- A small key/value bag for CLI bookkeeping that is neither domain data nor
-- sync configuration. Currently holds the last-activity heartbeat used by
-- in-terminal idle detection for active timers:
--   'last_activity' -> RFC3339 UTC timestamp of the most recent CLI activity
--
-- This table is intentionally excluded from sync (it is device-local state).
CREATE TABLE app_state (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
