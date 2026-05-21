//! Repository helpers for short-id allocation.

use rusqlite::{Connection, OptionalExtension, params};
use tock_core::domain::sid::SidKind;

use crate::Error;
use crate::repo::parse_u32;

/// Allocate and return the next SID for the given entity kind.
///
/// This increments the `sid_counters.next` value atomically inside
/// `SQLite` and returns the pre-increment value to the caller.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures and
/// [`crate::Error::NotFound`] if the counter row for `kind` is missing.
pub fn next_sid(conn: &Connection, kind: SidKind) -> Result<u32, Error> {
    let next_value: Option<i64> = conn
        .query_row(
            "UPDATE sid_counters
             SET next = next + 1
             WHERE kind = ?1
             RETURNING next - 1",
            params![kind.as_str()],
            |row| row.get(0),
        )
        .optional()?;

    let Some(next_value) = next_value else {
        return Err(Error::NotFound);
    };

    parse_u32(next_value)
}
