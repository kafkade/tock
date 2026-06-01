//! `CalDAV` error types.

/// Errors that can occur during `CalDAV` operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// iCalendar parse error.
    #[error("ical parse: {0}")]
    IcalParse(String),

    /// iCalendar serialization error.
    #[error("ical serialize: {0}")]
    IcalSerialize(String),

    /// Field mapping error (incompatible value).
    #[error("mapping: {0}")]
    Mapping(String),

    /// Transport / network error.
    #[error("transport: {0}")]
    Transport(String),

    /// Server returned HTTP error.
    #[error("http {status}: {body}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Response body (may be truncated).
        body: String,
    },

    /// `ETag` conflict (412 Precondition Failed).
    #[error("etag conflict on {href}: local etag {local_etag:?} rejected")]
    EtagConflict {
        /// Resource href.
        href: String,
        /// Local etag that was sent.
        local_etag: Option<String>,
    },

    /// Max retries exceeded during conflict resolution.
    #[error("max retries ({max}) exceeded for {href}")]
    MaxRetries {
        /// Resource href.
        href: String,
        /// How many attempts were made.
        max: u32,
    },

    /// Unknown or unsupported `CalDAV` feature.
    #[error("unsupported: {0}")]
    Unsupported(String),
}
