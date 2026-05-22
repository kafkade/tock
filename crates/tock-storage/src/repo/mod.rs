//! Repository layer — typed CRUD over `SQLite` tables.

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tock_core::Error as CoreError;
use uuid::Uuid;

use crate::Error;

pub mod area_repo;
pub mod focus_repo;
pub mod habit_repo;
pub mod heading_repo;
pub mod project_repo;
pub mod report_repo;
pub mod sid_repo;
pub mod tag_repo;
pub mod task_repo;
pub mod time_block_repo;
pub mod uda_repo;

pub(crate) const fn invalid_encoding() -> Error {
    Error::Core(CoreError::InvalidEncoding)
}

pub(crate) fn format_timestamp(timestamp: OffsetDateTime) -> Result<String, Error> {
    timestamp.format(&Rfc3339).map_err(|_| invalid_encoding())
}

pub(crate) fn parse_timestamp(raw: &str) -> Result<OffsetDateTime, Error> {
    OffsetDateTime::parse(raw, &Rfc3339).map_err(|_| invalid_encoding())
}

pub(crate) fn parse_optional_timestamp(raw: Option<&str>) -> Result<Option<OffsetDateTime>, Error> {
    raw.map(parse_timestamp).transpose()
}

pub(crate) fn parse_uuid_blob(raw: &[u8]) -> Result<Uuid, Error> {
    Uuid::from_slice(raw).map_err(|_| invalid_encoding())
}

pub(crate) fn parse_optional_uuid_blob(raw: Option<&[u8]>) -> Result<Option<Uuid>, Error> {
    raw.map(parse_uuid_blob).transpose()
}

pub(crate) fn uuid_to_blob(id: Uuid) -> Vec<u8> {
    id.as_bytes().to_vec()
}

pub(crate) const fn bool_to_int(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

pub(crate) const fn parse_bool(value: i64) -> Result<bool, Error> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid_encoding()),
    }
}

pub(crate) fn parse_u32(value: i64) -> Result<u32, Error> {
    u32::try_from(value).map_err(|_| invalid_encoding())
}
