//! Repository functions for habits.

use std::collections::HashSet;

use rusqlite::{Connection, Row, params};
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime, Time};
use tock_core::domain::cadence::ParsedCadence;
use tock_core::domain::habit::{
    Habit, HabitDirection, HabitEntry, HabitPatch, HabitSkip, NewHabit,
};
use tock_core::domain::sid::SidKind;
use uuid::Uuid;

use super::{
    bool_to_int, format_timestamp, parse_bool, parse_optional_timestamp, parse_optional_uuid_blob,
    parse_timestamp, parse_u32, parse_uuid_blob, uuid_to_blob,
};
use crate::Error;
use crate::repo::sid_repo;

const SELECT_HABIT_SQL: &str = "SELECT id, sid, title, identity, cue, craving, response, reward, direction, cadence, minimum, stack_after, stack_delay_s, area_id, project_id, level, xp, streak_current, streak_best, created_at, modified_at, archived_at FROM habits";
const SELECT_HABIT_ENTRY_SQL: &str =
    "SELECT id, habit_id, occurred_at, amount, notes, slip, source, created_at FROM habit_entries";
const SELECT_HABIT_SKIP_SQL: &str =
    "SELECT id, habit_id, date, kind, reason, created_at FROM habit_skips";
const XP_PER_SUCCESS: u32 = 1;
const LEVEL_THRESHOLDS: [u32; 7] = [0, 5, 13, 34, 89, 233, 610];

/// Insert a new habit row and return it.
///
/// Resolves any stacking parent SID to a habit UUID before insertion.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on database failures, [`crate::Error::NotFound`]
/// when a referenced stacking parent does not exist, and [`crate::Error::Core`] if
/// stored UUID or timestamp data is invalid.
pub fn insert(conn: &Connection, new: &NewHabit) -> Result<Habit, Error> {
    let id = Uuid::now_v7();
    let sid = sid_repo::next_sid(conn, SidKind::Habit)?;
    let created_at = OffsetDateTime::now_utc();
    let created_at_text = format_timestamp(created_at)?;
    let stack_after = resolve_stack_after_sid(conn, sid, new.stack_after)?;

    conn.execute(
        "INSERT INTO habits (
             id, sid, title, identity, cue, craving, response, reward, direction,
             cadence, minimum, stack_after, stack_delay_s, area_id, project_id,
             level, xp, streak_current, streak_best, reminders, accountability,
             source_task_id, created_at, modified_at, archived_at
         )
         VALUES (
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
             ?10, ?11, ?12, ?13, ?14, ?15,
             1, 0, 0, 0, '[]', NULL,
             NULL, ?16, ?17, NULL
         )",
        params![
            uuid_to_blob(id),
            i64::from(sid),
            new.title,
            new.identity,
            new.cue,
            new.craving,
            new.response,
            new.reward,
            new.direction.as_str(),
            new.cadence,
            new.minimum,
            stack_after.map(uuid_to_blob),
            i64::from(new.stack_delay_s),
            new.area_id.map(uuid_to_blob),
            new.project_id.map(uuid_to_blob),
            created_at_text,
            created_at_text,
        ],
    )?;

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

/// Fetch a habit by SID.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and [`crate::Error::Core`]
/// if stored UUID or timestamp data is invalid.
pub fn get_by_sid(conn: &Connection, sid: u32) -> Result<Option<Habit>, Error> {
    fetch_habit(conn, "sid = ?1", params![i64::from(sid)])
}

/// List habits ordered by SID ascending.
///
/// Archived habits are excluded unless `include_archived` is `true`.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and [`crate::Error::Core`]
/// if stored UUID or timestamp data is invalid.
pub fn list(conn: &Connection, include_archived: bool) -> Result<Vec<Habit>, Error> {
    let sql = if include_archived {
        format!("{SELECT_HABIT_SQL} ORDER BY sid ASC")
    } else {
        format!("{SELECT_HABIT_SQL} WHERE archived_at IS NULL ORDER BY sid ASC")
    };

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![])?;
    let mut habits = Vec::new();
    while let Some(row) = rows.next()? {
        habits.push(read_habit_row(row)?);
    }
    Ok(habits)
}

/// Apply a patch to an existing habit and return the updated row.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures, [`crate::Error::NotFound`]
/// if the habit or requested parent habit does not exist, and [`crate::Error::Core`]
/// if stored UUID or timestamp data is invalid.
pub fn update(conn: &Connection, sid: u32, patch: &HabitPatch) -> Result<Habit, Error> {
    let existing = get_by_sid(conn, sid)?.ok_or(Error::NotFound)?;
    let modified_at_text = format_timestamp(OffsetDateTime::now_utc())?;
    let stack_after = match patch.stack_after {
        Some(parent_sid) => resolve_stack_after_sid(conn, sid, parent_sid)?,
        None => existing.stack_after,
    };

    conn.execute(
        "UPDATE habits
         SET title = ?1,
             identity = ?2,
             cue = ?3,
             craving = ?4,
             response = ?5,
             reward = ?6,
             stack_after = ?7,
             stack_delay_s = ?8,
             modified_at = ?9
         WHERE sid = ?10",
        params![
            patch
                .title
                .clone()
                .unwrap_or_else(|| existing.title.clone()),
            patch
                .identity
                .clone()
                .unwrap_or_else(|| existing.identity.clone()),
            patch.cue.clone().unwrap_or_else(|| existing.cue.clone()),
            patch
                .craving
                .clone()
                .unwrap_or_else(|| existing.craving.clone()),
            patch
                .response
                .clone()
                .unwrap_or_else(|| existing.response.clone()),
            patch
                .reward
                .clone()
                .unwrap_or_else(|| existing.reward.clone()),
            stack_after.map(uuid_to_blob),
            i64::from(patch.stack_delay_s.unwrap_or(existing.stack_delay_s)),
            modified_at_text,
            i64::from(sid),
        ],
    )?;

    get_by_sid(conn, sid)?.ok_or(Error::NotFound)
}

/// Archive a habit by setting `archived_at`.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures, [`crate::Error::NotFound`]
/// if the habit does not exist, and [`crate::Error::Core`] if timestamps cannot be
/// formatted.
pub fn archive(conn: &Connection, sid: u32) -> Result<(), Error> {
    let now_text = format_timestamp(OffsetDateTime::now_utc())?;
    let rows_affected = conn.execute(
        "UPDATE habits SET archived_at = ?1, modified_at = ?2 WHERE sid = ?3",
        params![now_text, now_text, i64::from(sid)],
    )?;

    if rows_affected == 0 {
        return Err(Error::NotFound);
    }

    Ok(())
}

/// Hard-delete a habit row.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures and [`crate::Error::NotFound`]
/// if the habit does not exist.
pub fn delete(conn: &Connection, sid: u32) -> Result<(), Error> {
    let rows_affected =
        conn.execute("DELETE FROM habits WHERE sid = ?1", params![i64::from(sid)])?;
    if rows_affected == 0 {
        return Err(Error::NotFound);
    }
    Ok(())
}

/// Log a habit completion or slip and return the stored entry.
///
/// Successful entries increment XP. A slip on a break habit preserves XP.
/// Streaks are recalculated from the full entry and skip history after insert.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures, [`crate::Error::NotFound`]
/// if the habit does not exist, [`crate::Error::InvalidState`] on XP overflow,
/// and [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn log_entry(
    conn: &Connection,
    habit_sid: u32,
    amount: &str,
    notes: Option<&str>,
    slip: bool,
) -> Result<HabitEntry, Error> {
    let habit = get_by_sid(conn, habit_sid)?.ok_or(Error::NotFound)?;
    let id = Uuid::now_v7();
    let occurred_at = OffsetDateTime::now_utc();
    let occurred_at_text = format_timestamp(occurred_at)?;

    conn.execute(
        "INSERT INTO habit_entries (id, habit_id, occurred_at, amount, notes, slip, source, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'cli', ?7)",
        params![
            uuid_to_blob(id),
            uuid_to_blob(habit.id),
            occurred_at_text,
            amount,
            notes,
            bool_to_int(slip),
            occurred_at_text,
        ],
    )?;

    let xp = if slip && habit.direction == HabitDirection::Break {
        habit.xp
    } else {
        habit
            .xp
            .checked_add(XP_PER_SUCCESS)
            .ok_or(Error::InvalidState("habit xp overflow"))?
    };
    let level = level_for_xp(xp);
    let modified_at_text = format_timestamp(occurred_at)?;

    conn.execute(
        "UPDATE habits
         SET xp = ?1,
             level = ?2,
             modified_at = ?3
         WHERE sid = ?4",
        params![
            i64::from(xp),
            i64::from(level),
            modified_at_text,
            i64::from(habit_sid),
        ],
    )?;

    recalculate_streak(conn, habit_sid)?;
    get_entry_by_id(conn, id)?.ok_or(Error::NotFound)
}

/// Log an entry for a past date.
///
/// This is useful for catching up on missed logs without affecting the entry amount
/// or notes schema.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures, [`crate::Error::NotFound`]
/// if the habit does not exist, [`crate::Error::InvalidState`] when the supplied date
/// is invalid or XP overflows, and [`crate::Error::Core`] if stored UUID or timestamp
/// data is invalid.
pub fn log_backfill(
    conn: &Connection,
    habit_sid: u32,
    date: &str,
    amount: &str,
    notes: Option<&str>,
) -> Result<HabitEntry, Error> {
    let habit = get_by_sid(conn, habit_sid)?.ok_or(Error::NotFound)?;
    let occurred_date = parse_iso_date(date)?;
    let id = Uuid::now_v7();
    let occurred_at = PrimitiveDateTime::new(occurred_date, Time::MIDNIGHT).assume_utc();
    let occurred_at_text = format_timestamp(occurred_at)?;
    let created_at_text = format_timestamp(OffsetDateTime::now_utc())?;

    conn.execute(
        "INSERT INTO habit_entries (id, habit_id, occurred_at, amount, notes, slip, source, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 'cli', ?6)",
        params![
            uuid_to_blob(id),
            uuid_to_blob(habit.id),
            occurred_at_text,
            amount,
            notes,
            created_at_text,
        ],
    )?;

    let xp = habit
        .xp
        .checked_add(XP_PER_SUCCESS)
        .ok_or(Error::InvalidState("habit xp overflow"))?;
    let level = level_for_xp(xp);
    let modified_at_text = format_timestamp(OffsetDateTime::now_utc())?;

    conn.execute(
        "UPDATE habits
         SET xp = ?1,
             level = ?2,
             modified_at = ?3
         WHERE sid = ?4",
        params![
            i64::from(xp),
            i64::from(level),
            modified_at_text,
            i64::from(habit_sid),
        ],
    )?;

    recalculate_streak(conn, habit_sid)?;
    get_entry_by_id(conn, id)?.ok_or(Error::NotFound)
}

/// Recalculate the streak for a habit based on its entries and cadence.
///
/// Grace days recorded in `habit_skips` count as completed days for streak purposes.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query or write failures, [`crate::Error::NotFound`]
/// if the habit does not exist, and [`crate::Error::Core`] if stored UUID or timestamp
/// data is invalid.
pub fn recalculate_streak(conn: &Connection, habit_sid: u32) -> Result<Habit, Error> {
    let habit = get_by_sid(conn, habit_sid)?.ok_or(Error::NotFound)?;
    let entries = get_entries(conn, habit_sid)?;
    let skips = get_skips(conn, habit.id)?;
    let cadence = ParsedCadence::from_json(&habit.cadence);
    let entry_dates: HashSet<_> = entries
        .iter()
        .filter(|entry| !entry.slip)
        .map(|entry| date_to_iso_string(entry.occurred_at.date()))
        .collect();
    let slip_dates: HashSet<_> = entries
        .iter()
        .filter(|entry| entry.slip)
        .map(|entry| date_to_iso_string(entry.occurred_at.date()))
        .collect();
    let grace_dates: HashSet<_> = skips.into_iter().map(|skip| skip.date).collect();
    let today = OffsetDateTime::now_utc().date();
    let mut streak = 0_u32;
    let mut date = today;

    for _ in 0..365_u16 {
        let date_str = date_to_iso_string(date);
        let was_due = cadence.as_ref().is_none_or(|parsed| parsed.is_due_on(date));
        if !was_due {
            date -= time::Duration::days(1);
            continue;
        }
        if habit.direction == HabitDirection::Break && slip_dates.contains(&date_str) {
            break;
        }
        if entry_dates.contains(&date_str) || grace_dates.contains(&date_str) {
            streak += 1;
            date -= time::Duration::days(1);
            continue;
        }
        break;
    }

    let best = streak.max(habit.streak_best);
    let now_str = format_timestamp(OffsetDateTime::now_utc())?;
    conn.execute(
        "UPDATE habits SET streak_current = ?1, streak_best = ?2, modified_at = ?3 WHERE sid = ?4",
        params![
            i64::from(streak),
            i64::from(best),
            now_str,
            i64::from(habit_sid),
        ],
    )?;
    get_by_sid(conn, habit_sid)?.ok_or(Error::NotFound)
}

/// List log entries for a habit ordered by `occurred_at` descending.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures, [`crate::Error::NotFound`]
/// if the habit does not exist, and [`crate::Error::Core`] if stored UUID or timestamp
/// data is invalid.
pub fn get_entries(conn: &Connection, habit_sid: u32) -> Result<Vec<HabitEntry>, Error> {
    let habit = get_by_sid(conn, habit_sid)?.ok_or(Error::NotFound)?;
    let sql = format!("{SELECT_HABIT_ENTRY_SQL} WHERE habit_id = ?1 ORDER BY occurred_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![uuid_to_blob(habit.id)])?;
    let mut entries = Vec::new();
    while let Some(row) = rows.next()? {
        entries.push(read_habit_entry_row(row)?);
    }
    Ok(entries)
}

/// Get recorded skip or freeze days for a habit.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and [`crate::Error::Core`]
/// if stored UUID or timestamp data is invalid.
pub fn get_skips(conn: &Connection, habit_id: Uuid) -> Result<Vec<HabitSkip>, Error> {
    let sql = format!("{SELECT_HABIT_SKIP_SQL} WHERE habit_id = ?1 ORDER BY date DESC");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![uuid_to_blob(habit_id)])?;
    let mut skips = Vec::new();
    while let Some(row) = rows.next()? {
        skips.push(read_habit_skip_row(row)?);
    }
    Ok(skips)
}

/// Add a skip or freeze day for a habit.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failures, [`crate::Error::NotFound`]
/// if the habit does not exist, [`crate::Error::InvalidState`] when the date or kind
/// is invalid, and [`crate::Error::Core`] if stored UUID or timestamp data is invalid.
pub fn add_skip(
    conn: &Connection,
    habit_sid: u32,
    date: &str,
    kind: &str,
    reason: Option<&str>,
) -> Result<(), Error> {
    let habit = get_by_sid(conn, habit_sid)?.ok_or(Error::NotFound)?;
    if !matches!(kind, "skip" | "freeze") {
        return Err(Error::InvalidState(
            "habit skip kind must be skip or freeze",
        ));
    }
    let skip_date = parse_iso_date(date)?;
    let created_at_text = format_timestamp(OffsetDateTime::now_utc())?;

    conn.execute(
        "INSERT INTO habit_skips (id, habit_id, date, kind, reason, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            uuid_to_blob(Uuid::now_v7()),
            uuid_to_blob(habit.id),
            date_to_iso_string(skip_date),
            kind,
            reason,
            created_at_text,
        ],
    )?;

    let _ = recalculate_streak(conn, habit_sid)?;
    Ok(())
}

/// List habits stacked after the given parent habit UUID.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on query failures and [`crate::Error::Core`]
/// if stored UUID or timestamp data is invalid.
pub fn get_stacked_habits(conn: &Connection, parent_id: Uuid) -> Result<Vec<Habit>, Error> {
    let sql = format!("{SELECT_HABIT_SQL} WHERE stack_after = ?1 ORDER BY sid ASC");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![uuid_to_blob(parent_id)])?;
    let mut habits = Vec::new();
    while let Some(row) = rows.next()? {
        habits.push(read_habit_row(row)?);
    }
    Ok(habits)
}

fn fetch_habit<P>(conn: &Connection, filter: &str, params: P) -> Result<Option<Habit>, Error>
where
    P: rusqlite::Params,
{
    let sql = format!("{SELECT_HABIT_SQL} WHERE {filter}");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params)?;
    if let Some(row) = rows.next()? {
        return Ok(Some(read_habit_row(row)?));
    }
    Ok(None)
}

fn get_entry_by_id(conn: &Connection, id: Uuid) -> Result<Option<HabitEntry>, Error> {
    let sql = format!("{SELECT_HABIT_ENTRY_SQL} WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![uuid_to_blob(id)])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(read_habit_entry_row(row)?));
    }
    Ok(None)
}

fn resolve_stack_after_sid(
    conn: &Connection,
    current_sid: u32,
    stack_after: Option<u32>,
) -> Result<Option<Uuid>, Error> {
    let Some(parent_sid) = stack_after else {
        return Ok(None);
    };
    if parent_sid == current_sid {
        return Err(Error::InvalidState("habit cannot stack after itself"));
    }
    let parent = get_by_sid(conn, parent_sid)?.ok_or(Error::NotFound)?;
    Ok(Some(parent.id))
}

fn level_for_xp(xp: u32) -> u32 {
    let mut level = 1_u32;
    for (index, threshold) in LEVEL_THRESHOLDS.iter().enumerate() {
        if xp >= *threshold {
            level = u32::try_from(index + 1).unwrap_or(7);
        }
    }
    level
}

fn parse_habit_direction(raw: &str) -> Result<HabitDirection, Error> {
    HabitDirection::from_str_opt(raw).ok_or_else(super::invalid_encoding)
}

fn parse_iso_date(raw: &str) -> Result<time::Date, Error> {
    time::Date::parse(raw, &format_description!("[year]-[month]-[day]"))
        .map_err(|_| Error::InvalidState("habit date must be YYYY-MM-DD"))
}

fn date_to_iso_string(date: time::Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn read_habit_entry_row(row: &Row<'_>) -> Result<HabitEntry, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    let habit_id_bytes: Vec<u8> = row.get("habit_id")?;
    let slip_raw: i64 = row.get("slip")?;

    Ok(HabitEntry {
        id: parse_uuid_blob(&id_bytes)?,
        habit_id: parse_uuid_blob(&habit_id_bytes)?,
        occurred_at: parse_timestamp(&row.get::<_, String>("occurred_at")?)?,
        amount: row.get("amount")?,
        notes: row.get("notes")?,
        slip: parse_bool(slip_raw)?,
        source: row.get("source")?,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
    })
}

fn read_habit_skip_row(row: &Row<'_>) -> Result<HabitSkip, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    let habit_id_bytes: Vec<u8> = row.get("habit_id")?;

    Ok(HabitSkip {
        id: parse_uuid_blob(&id_bytes)?,
        habit_id: parse_uuid_blob(&habit_id_bytes)?,
        date: row.get("date")?,
        kind: row.get("kind")?,
        reason: row.get("reason")?,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
    })
}

fn read_habit_row(row: &Row<'_>) -> Result<Habit, Error> {
    let id_bytes: Vec<u8> = row.get("id")?;
    let sid_value: i64 = row.get("sid")?;
    let direction_raw: String = row.get("direction")?;
    let stack_delay_raw: i64 = row.get("stack_delay_s")?;
    let level_raw: i64 = row.get("level")?;
    let xp_raw: i64 = row.get("xp")?;
    let streak_current_raw: i64 = row.get("streak_current")?;
    let streak_best_raw: i64 = row.get("streak_best")?;

    Ok(Habit {
        id: parse_uuid_blob(&id_bytes)?,
        sid: parse_u32(sid_value)?,
        title: row.get("title")?,
        identity: row.get("identity")?,
        cue: row.get("cue")?,
        craving: row.get("craving")?,
        response: row.get("response")?,
        reward: row.get("reward")?,
        direction: parse_habit_direction(&direction_raw)?,
        cadence: row.get("cadence")?,
        minimum: row.get("minimum")?,
        stack_after: parse_optional_uuid_blob(
            row.get::<_, Option<Vec<u8>>>("stack_after")?.as_deref(),
        )?,
        stack_delay_s: parse_u32(stack_delay_raw)?,
        area_id: parse_optional_uuid_blob(row.get::<_, Option<Vec<u8>>>("area_id")?.as_deref())?,
        project_id: parse_optional_uuid_blob(
            row.get::<_, Option<Vec<u8>>>("project_id")?.as_deref(),
        )?,
        level: parse_u32(level_raw)?,
        xp: parse_u32(xp_raw)?,
        streak_current: parse_u32(streak_current_raw)?,
        streak_best: parse_u32(streak_best_raw)?,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
        modified_at: parse_timestamp(&row.get::<_, String>("modified_at")?)?,
        archived_at: parse_optional_timestamp(
            row.get::<_, Option<String>>("archived_at")?.as_deref(),
        )?,
    })
}

/// Get the reminder configuration for a habit.
///
/// Reads the `reminders` JSON column from the habits table.
///
/// # Errors
/// Returns [`crate::Error::NotFound`] if the habit doesn't exist.
pub fn get_reminders(
    conn: &Connection,
    habit_sid: u32,
) -> Result<Vec<tock_core::domain::habit::Reminder>, Error> {
    let raw: String = conn.query_row(
        "SELECT reminders FROM habits WHERE sid = ?1",
        params![i64::from(habit_sid)],
        |r| r.get(0),
    )?;
    Ok(tock_core::domain::habit::Reminder::from_json_array(&raw))
}

/// Set (replace) the reminders for a habit.
///
/// # Errors
/// Returns [`crate::Error::Sqlite`] on write failure.
pub fn set_reminders(
    conn: &Connection,
    habit_sid: u32,
    reminders: &[tock_core::domain::habit::Reminder],
) -> Result<(), Error> {
    let json = tock_core::domain::habit::Reminder::to_json_array(reminders);
    let now = format_timestamp(OffsetDateTime::now_utc())?;
    conn.execute(
        "UPDATE habits SET reminders = ?1, modified_at = ?2 WHERE sid = ?3",
        params![json, now, i64::from(habit_sid)],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use rusqlite::Connection;
    use time::OffsetDateTime;
    use tock_core::domain::habit::{HabitDirection, HabitPatch, NewHabit};

    use super::{
        add_skip, archive, date_to_iso_string, delete, get_by_sid, get_entries, get_skips,
        get_stacked_habits, insert, log_backfill, log_entry, update,
    };
    use crate::migrations;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        migrations::migrate(&mut conn).expect("migrate");
        conn
    }

    fn new_habit(title: &str) -> NewHabit {
        NewHabit {
            title: title.to_string(),
            identity: None,
            cue: None,
            craving: None,
            response: None,
            reward: None,
            direction: HabitDirection::Build,
            cadence: String::from("\"daily\""),
            minimum: String::from("\"boolean\""),
            stack_after: None,
            stack_delay_s: 0,
            area_id: None,
            project_id: None,
        }
    }

    fn iso_date(offset_days: i64) -> String {
        let date = OffsetDateTime::now_utc().date() + time::Duration::days(offset_days);
        date_to_iso_string(date)
    }

    #[test]
    fn insert_update_and_stack_roundtrip() {
        let conn = test_conn();
        let parent = insert(&conn, &new_habit("Coffee")).expect("insert parent habit");
        let mut child_input = new_habit("Read");
        child_input.identity = Some(String::from("I am a reader"));
        child_input.stack_after = Some(parent.sid);
        child_input.stack_delay_s = 60;
        let child = insert(&conn, &child_input).expect("insert child habit");

        assert_eq!(child.stack_after, Some(parent.id));
        assert_eq!(
            get_stacked_habits(&conn, parent.id)
                .expect("stacked habits")
                .len(),
            1
        );

        let patch = HabitPatch {
            title: Some(String::from("Read 10 pages")),
            identity: Some(None),
            cue: Some(Some(String::from("After coffee"))),
            craving: None,
            response: Some(Some(String::from("Open book"))),
            reward: Some(Some(String::from("Tea"))),
            stack_after: Some(None),
            stack_delay_s: Some(0),
        };
        let updated = update(&conn, child.sid, &patch).expect("update habit");
        assert_eq!(updated.title, "Read 10 pages");
        assert_eq!(updated.identity, None);
        assert_eq!(updated.stack_after, None);
        assert_eq!(updated.cue.as_deref(), Some("After coffee"));
    }

    #[test]
    fn log_entries_update_progress_and_break_slips_reset_streak() {
        let conn = test_conn();
        let habit = insert(&conn, &new_habit("Meditate")).expect("insert habit");

        let first = log_entry(&conn, habit.sid, "true", None, false).expect("log first entry");
        let second =
            log_entry(&conn, habit.sid, "true", Some("nice"), false).expect("log second entry");
        let habit = get_by_sid(&conn, habit.sid)
            .expect("fetch habit")
            .expect("habit exists");
        assert_eq!(habit.xp, 2);
        assert_eq!(habit.level, 1);
        assert_eq!(habit.streak_current, 1);
        assert_eq!(habit.streak_best, 1);

        let entries = get_entries(&conn, habit.sid).expect("get entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, second.id);
        assert_eq!(entries[1].id, first.id);
        assert_eq!(entries[0].notes.as_deref(), Some("nice"));

        let mut break_habit = new_habit("No doomscrolling");
        break_habit.direction = HabitDirection::Break;
        let break_habit = insert(&conn, &break_habit).expect("insert break habit");
        log_entry(&conn, break_habit.sid, "true", None, false).expect("log avoided slip");
        let break_habit =
            log_entry(&conn, break_habit.sid, "true", Some("slipped"), true).expect("log slip");
        assert!(break_habit.slip);
        let break_state = get_by_sid(&conn, 2)
            .expect("fetch break habit")
            .expect("break habit exists");
        assert_eq!(break_state.xp, 1);
        assert_eq!(break_state.streak_current, 0);
        assert_eq!(break_state.streak_best, 1);
    }

    #[test]
    fn skips_and_backfills_extend_daily_streaks() {
        let conn = test_conn();
        let habit = insert(&conn, &new_habit("Read")).expect("insert habit");
        let two_days_ago = iso_date(-2);
        let yesterday = iso_date(-1);

        let backfill =
            log_backfill(&conn, habit.sid, &two_days_ago, "true", None).expect("backfill entry");
        assert_eq!(
            date_to_iso_string(backfill.occurred_at.date()),
            two_days_ago
        );

        add_skip(&conn, habit.sid, &yesterday, "skip", Some("travel")).expect("add skip day");
        log_entry(&conn, habit.sid, "true", None, false).expect("log today");

        let habit = get_by_sid(&conn, habit.sid)
            .expect("fetch habit")
            .expect("habit exists");
        assert_eq!(habit.streak_current, 3);
        assert_eq!(habit.streak_best, 3);
        assert_eq!(habit.xp, 2);

        let skips = get_skips(&conn, habit.id).expect("get skips");
        assert_eq!(skips.len(), 1);
        assert_eq!(skips[0].kind, "skip");
        assert_eq!(skips[0].date, yesterday);
    }

    #[test]
    fn archive_and_delete_work() {
        let conn = test_conn();
        let habit = insert(&conn, &new_habit("Stretch")).expect("insert habit");
        archive(&conn, habit.sid).expect("archive habit");
        assert!(
            get_by_sid(&conn, habit.sid)
                .expect("fetch archived habit")
                .expect("archived habit exists")
                .archived_at
                .is_some()
        );
        delete(&conn, habit.sid).expect("delete habit");
        assert!(
            get_by_sid(&conn, habit.sid)
                .expect("fetch deleted habit")
                .is_none()
        );
    }
}
