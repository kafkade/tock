CREATE TABLE uda_definitions (
    key       TEXT PRIMARY KEY,
    type      TEXT NOT NULL CHECK (type IN ('string','number','date','boolean')),
    label     TEXT,
    "default" TEXT
);
