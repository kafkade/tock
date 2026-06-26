//! Registry of syncable domain tables.
//!
//! Each entry describes how one `SQLite` table maps onto the event-sourced
//! sync substrate: which [`EntityKind`] it carries, how to derive a
//! stable 16-byte sync id from its primary key, which columns are
//! local-only (excluded from the synced snapshot), and whether new rows
//! need a locally-allocated short id (SID).

use tock_core::domain::sid::SidKind;
use tock_core::event::EntityKind;

/// How a table's primary key maps to a sync id.
pub enum Key {
    /// A single 16-byte blob column whose bytes are used directly as the
    /// sync id (uuid `id` columns and the 16-byte `device_id`).
    Uuid(&'static str),
    /// A composite key; the sync id is `uuidv5` of the concatenated key
    /// column values.
    Composite(&'static [&'static str]),
}

/// Descriptor for one syncable table.
pub struct SyncTable {
    /// Entity kind carried by events for this table.
    pub kind: EntityKind,
    /// `SQLite` table name.
    pub table: &'static str,
    /// Primary-key mapping.
    pub key: Key,
    /// Columns excluded from the synced snapshot (local-only / derived).
    pub skip: &'static [&'static str],
    /// SID kind to allocate for brand-new rows (`None` if the table has
    /// no `sid` column).
    pub sid: Option<SidKind>,
}

impl SyncTable {
    /// Key column names for this table.
    pub(crate) fn key_columns(&self) -> Vec<&'static str> {
        match &self.key {
            Key::Uuid(c) => vec![*c],
            Key::Composite(cs) => cs.to_vec(),
        }
    }
}

/// All syncable tables, ordered parents-before-children so materializing
/// in registry order is well-formed even if a future build enables
/// foreign-key enforcement.
pub const TABLES: &[SyncTable] = &[
    SyncTable {
        kind: EntityKind::Device,
        table: "devices",
        key: Key::Uuid("device_id"),
        skip: &[],
        sid: None,
    },
    SyncTable {
        kind: EntityKind::Area,
        table: "areas",
        key: Key::Uuid("id"),
        skip: &[],
        sid: None,
    },
    SyncTable {
        kind: EntityKind::Project,
        table: "projects",
        key: Key::Uuid("id"),
        skip: &["sid"],
        sid: Some(SidKind::Project),
    },
    SyncTable {
        kind: EntityKind::Heading,
        table: "headings",
        key: Key::Uuid("id"),
        skip: &[],
        sid: None,
    },
    SyncTable {
        kind: EntityKind::Tag,
        table: "tags",
        key: Key::Uuid("id"),
        skip: &[],
        sid: None,
    },
    SyncTable {
        kind: EntityKind::Task,
        table: "tasks",
        key: Key::Uuid("id"),
        skip: &["sid", "urgency_cache"],
        sid: Some(SidKind::Task),
    },
    SyncTable {
        kind: EntityKind::TagLink,
        table: "entity_tags",
        key: Key::Composite(&["entity_id", "entity_kind", "tag_id"]),
        skip: &[],
        sid: None,
    },
    SyncTable {
        kind: EntityKind::Annotation,
        table: "annotations",
        key: Key::Uuid("id"),
        skip: &[],
        sid: None,
    },
    SyncTable {
        kind: EntityKind::TimeBlock,
        table: "time_blocks",
        key: Key::Uuid("id"),
        skip: &["sid"],
        sid: Some(SidKind::Block),
    },
    SyncTable {
        kind: EntityKind::FocusSession,
        table: "focus_sessions",
        key: Key::Uuid("id"),
        skip: &["sid"],
        sid: Some(SidKind::Focus),
    },
    SyncTable {
        kind: EntityKind::Habit,
        table: "habits",
        key: Key::Uuid("id"),
        skip: &["sid"],
        sid: Some(SidKind::Habit),
    },
    SyncTable {
        kind: EntityKind::HabitEntry,
        table: "habit_entries",
        key: Key::Uuid("id"),
        skip: &[],
        sid: None,
    },
    SyncTable {
        kind: EntityKind::HabitSkip,
        table: "habit_skips",
        key: Key::Uuid("id"),
        skip: &[],
        sid: None,
    },
];

/// Find the table descriptor for an entity kind.
pub fn table_for(kind: EntityKind) -> Option<&'static SyncTable> {
    TABLES.iter().find(|t| t.kind == kind)
}
