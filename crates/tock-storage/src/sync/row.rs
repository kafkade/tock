//! Generic row snapshot: serialize a domain-table row to a canonical,
//! self-describing byte frame, and materialize one back.
//!
//! The frame is the AEAD plaintext payload of a sync event. It is a
//! sorted list of `(column_name, value)` pairs so that two devices
//! produce byte-identical frames for identical logical rows (stable
//! change detection + dedup).

use rusqlite::Connection;
use rusqlite::params_from_iter;
use rusqlite::types::Value;
use uuid::Uuid;

use crate::Error;
use crate::repo::sid_repo;
use crate::sync::registry::{Key, SyncTable};

/// Namespace uuid for deriving composite-key sync ids.
const COMPOSITE_NS: Uuid = Uuid::from_bytes([
    0x6f, 0x63, 0x6b, 0x73, 0x79, 0x6e, 0x63, 0x2d, 0x63, 0x6f, 0x6d, 0x70, 0x6b, 0x65, 0x79, 0x31,
]);

/// A snapshotted row: its sync id, canonical content frame, and decoded
/// column set.
pub struct RowSnap {
    /// Stable 16-byte sync id.
    pub sync_id: Uuid,
    /// Canonical serialization of `columns` (the event payload).
    pub content: Vec<u8>,
    /// Decoded `(column, value)` pairs (excludes skipped columns).
    pub columns: Vec<(String, Value)>,
}

/// Snapshot every current row of a table.
pub fn snapshot_table(conn: &Connection, table: &SyncTable) -> Result<Vec<RowSnap>, Error> {
    let sql = format!("SELECT * FROM {}", table.table);
    let mut stmt = conn.prepare(&sql)?;
    let col_names: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut columns: Vec<(String, Value)> = Vec::with_capacity(col_names.len());
        for (i, name) in col_names.iter().enumerate() {
            if table.skip.contains(&name.as_str()) {
                continue;
            }
            let value: Value = row.get(i)?;
            columns.push((name.clone(), value));
        }
        let sync_id = compute_sync_id(table, &columns)?;
        let content = encode_columns(&columns);
        out.push(RowSnap {
            sync_id,
            content,
            columns,
        });
    }
    Ok(out)
}

/// Derive the sync id for a row from its key columns.
pub fn compute_sync_id(table: &SyncTable, columns: &[(String, Value)]) -> Result<Uuid, Error> {
    match &table.key {
        Key::Uuid(col) => {
            let v = column_value(columns, col)?;
            if let Value::Blob(b) = v {
                let arr: [u8; 16] = b
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::EventLogIntegrity)?;
                Ok(Uuid::from_bytes(arr))
            } else {
                Err(Error::EventLogIntegrity)
            }
        }
        Key::Composite(keys) => {
            let mut seed = Vec::new();
            for k in *keys {
                let v = column_value(columns, k)?;
                put_value(&mut seed, v);
            }
            Ok(Uuid::new_v5(&COMPOSITE_NS, &seed))
        }
    }
}

/// Extract the `(column, value)` pairs that make up a table's key.
pub fn key_columns_of(
    table: &SyncTable,
    columns: &[(String, Value)],
) -> Result<Vec<(String, Value)>, Error> {
    let mut out = Vec::new();
    for k in table.key_columns() {
        let v = column_value(columns, k)?;
        out.push(((*k).to_string(), v.clone()));
    }
    Ok(out)
}

fn column_value<'a>(columns: &'a [(String, Value)], name: &str) -> Result<&'a Value, Error> {
    columns
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
        .ok_or(Error::EventLogIntegrity)
}

/// Names of columns whose values differ between two decoded snapshots.
pub fn changed_fields(old: &[(String, Value)], new: &[(String, Value)]) -> Vec<String> {
    let mut fields = Vec::new();
    for (name, nv) in new {
        let differs = old
            .iter()
            .find(|(on, _)| on == name)
            .is_none_or(|(_, ov)| ov != nv);
        if differs {
            fields.push(name.clone());
        }
    }
    fields
}

// ── Materialization ──────────────────────────────────────────────────

/// Insert or update a row from a decoded column set (full snapshot).
///
/// For SID-bearing tables the local `sid` is preserved on update and
/// freshly allocated on insert, so short ids stay device-local.
pub fn upsert_row(
    conn: &Connection,
    table: &SyncTable,
    columns: &[(String, Value)],
) -> Result<(), Error> {
    if let Some(sid_kind) = table.sid {
        if row_exists(conn, table, columns)? {
            update_row(conn, table, columns)
        } else {
            let sid = sid_repo::next_sid(conn, sid_kind)?;
            insert_row(conn, table, columns, Some(("sid", i64::from(sid))))
        }
    } else {
        insert_or_replace_row(conn, table, columns)
    }
}

/// Delete a row addressed by its decoded key columns.
pub fn delete_row(
    conn: &Connection,
    table: &SyncTable,
    key_columns: &[(String, Value)],
) -> Result<(), Error> {
    let (where_sql, values) = key_predicate(table, key_columns)?;
    let sql = format!("DELETE FROM {} WHERE {where_sql}", table.table);
    conn.execute(&sql, params_from_iter(values.iter()))?;
    Ok(())
}

/// Load the current synced columns of a single row addressed by its key
/// columns (excludes skipped columns). `None` if the row is absent.
pub fn load_row(
    conn: &Connection,
    table: &SyncTable,
    key_columns: &[(String, Value)],
) -> Result<Option<Vec<(String, Value)>>, Error> {
    let (where_sql, values) = key_predicate(table, key_columns)?;
    let sql = format!("SELECT * FROM {} WHERE {where_sql} LIMIT 1", table.table);
    let mut stmt = conn.prepare(&sql)?;
    let col_names: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let mut rows = stmt.query(params_from_iter(values.iter()))?;
    match rows.next()? {
        Some(row) => {
            let mut columns = Vec::with_capacity(col_names.len());
            for (i, name) in col_names.iter().enumerate() {
                if table.skip.contains(&name.as_str()) {
                    continue;
                }
                let value: Value = row.get(i)?;
                columns.push((name.clone(), value));
            }
            Ok(Some(columns))
        }
        None => Ok(None),
    }
}

fn row_exists(
    conn: &Connection,
    table: &SyncTable,
    columns: &[(String, Value)],
) -> Result<bool, Error> {
    let key_cols = key_columns_of(table, columns)?;
    let (where_sql, values) = key_predicate(table, &key_cols)?;
    let sql = format!("SELECT 1 FROM {} WHERE {where_sql} LIMIT 1", table.table);
    let mut stmt = conn.prepare(&sql)?;
    let exists = stmt.exists(params_from_iter(values.iter()))?;
    Ok(exists)
}

fn insert_or_replace_row(
    conn: &Connection,
    table: &SyncTable,
    columns: &[(String, Value)],
) -> Result<(), Error> {
    let names: Vec<&str> = columns.iter().map(|(n, _)| n.as_str()).collect();
    let placeholders = (1..=names.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT OR REPLACE INTO {} ({}) VALUES ({placeholders})",
        table.table,
        names.join(", "),
    );
    let values: Vec<&Value> = columns.iter().map(|(_, v)| v).collect();
    conn.execute(&sql, params_from_iter(values.iter()))?;
    Ok(())
}

fn insert_row(
    conn: &Connection,
    table: &SyncTable,
    columns: &[(String, Value)],
    extra: Option<(&str, i64)>,
) -> Result<(), Error> {
    let mut names: Vec<String> = columns.iter().map(|(n, _)| n.clone()).collect();
    let mut values: Vec<Value> = columns.iter().map(|(_, v)| v.clone()).collect();
    if let Some((col, val)) = extra {
        names.push(col.to_string());
        values.push(Value::Integer(val));
    }
    let placeholders = (1..=names.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({placeholders})",
        table.table,
        names.join(", "),
    );
    conn.execute(&sql, params_from_iter(values.iter()))?;
    Ok(())
}

fn update_row(
    conn: &Connection,
    table: &SyncTable,
    columns: &[(String, Value)],
) -> Result<(), Error> {
    let keys = table.key_columns();
    let set_cols: Vec<&(String, Value)> = columns
        .iter()
        .filter(|(n, _)| !keys.contains(&n.as_str()))
        .collect();
    if set_cols.is_empty() {
        return Ok(());
    }
    let mut values: Vec<Value> = Vec::with_capacity(columns.len());
    let mut idx = 1;
    let mut set_sql = Vec::new();
    for (name, value) in &set_cols {
        set_sql.push(format!("{name} = ?{idx}"));
        values.push(value.clone());
        idx += 1;
    }
    let mut where_sql = Vec::new();
    for k in keys {
        let v = column_value(columns, k)?;
        where_sql.push(format!("{k} = ?{idx}"));
        values.push(v.clone());
        idx += 1;
    }
    let sql = format!(
        "UPDATE {} SET {} WHERE {}",
        table.table,
        set_sql.join(", "),
        where_sql.join(" AND "),
    );
    conn.execute(&sql, params_from_iter(values.iter()))?;
    Ok(())
}

fn key_predicate(
    table: &SyncTable,
    key_columns: &[(String, Value)],
) -> Result<(String, Vec<Value>), Error> {
    let mut clauses = Vec::new();
    let mut values = Vec::new();
    for (offset, k) in table.key_columns().into_iter().enumerate() {
        let idx = offset + 1;
        let v = column_value(key_columns, k)?;
        clauses.push(format!("{k} = ?{idx}"));
        values.push(v.clone());
    }
    Ok((clauses.join(" AND "), values))
}

// ── Canonical column codec ───────────────────────────────────────────

/// Serialize a column set to a canonical, sorted byte frame.
pub fn encode_columns(columns: &[(String, Value)]) -> Vec<u8> {
    let mut sorted: Vec<&(String, Value)> = columns.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = Vec::new();
    let count = u32::try_from(sorted.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&count.to_be_bytes());
    for (name, value) in sorted {
        put_lp(&mut out, name.as_bytes());
        put_value(&mut out, value);
    }
    out
}

/// Parse a frame produced by [`encode_columns`].
pub fn decode_columns(bytes: &[u8]) -> Result<Vec<(String, Value)>, Error> {
    let mut cur = Cursor { bytes, off: 0 };
    let count = cur.u32()?;
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name_bytes = cur.lp()?;
        let name = String::from_utf8(name_bytes).map_err(|_| Error::EventLogIntegrity)?;
        let value = cur.value()?;
        out.push((name, value));
    }
    Ok(out)
}

fn put_lp(out: &mut Vec<u8>, bytes: &[u8]) {
    let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
}

fn put_value(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => out.push(0),
        Value::Integer(i) => {
            out.push(1);
            out.extend_from_slice(&i.to_be_bytes());
        }
        Value::Real(f) => {
            out.push(2);
            out.extend_from_slice(&f.to_bits().to_be_bytes());
        }
        Value::Text(s) => {
            out.push(3);
            put_lp(out, s.as_bytes());
        }
        Value::Blob(b) => {
            out.push(4);
            put_lp(out, b);
        }
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    off: usize,
}

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let end = self.off.checked_add(n).ok_or(Error::EventLogIntegrity)?;
        if end > self.bytes.len() {
            return Err(Error::EventLogIntegrity);
        }
        let s = &self.bytes[self.off..end];
        self.off = end;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let b: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| Error::EventLogIntegrity)?;
        Ok(u32::from_be_bytes(b))
    }

    fn lp(&mut self) -> Result<Vec<u8>, Error> {
        let len = self.u32()? as usize;
        Ok(self.take(len)?.to_vec())
    }

    fn value(&mut self) -> Result<Value, Error> {
        let tag = self.take(1)?[0];
        match tag {
            0 => Ok(Value::Null),
            1 => {
                let b: [u8; 8] = self
                    .take(8)?
                    .try_into()
                    .map_err(|_| Error::EventLogIntegrity)?;
                Ok(Value::Integer(i64::from_be_bytes(b)))
            }
            2 => {
                let b: [u8; 8] = self
                    .take(8)?
                    .try_into()
                    .map_err(|_| Error::EventLogIntegrity)?;
                Ok(Value::Real(f64::from_bits(u64::from_be_bytes(b))))
            }
            3 => {
                let raw = self.lp()?;
                let s = String::from_utf8(raw).map_err(|_| Error::EventLogIntegrity)?;
                Ok(Value::Text(s))
            }
            4 => Ok(Value::Blob(self.lp()?)),
            _ => Err(Error::EventLogIntegrity),
        }
    }
}
