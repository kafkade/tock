//! # tock-caldav
//!
//! `CalDAV` bidirectional sync for tock. Maps tasks to `VTODO` and time
//! blocks to `VEVENT`, syncs with `CalDAV` servers (`Nextcloud`, `Radicale`,
//! `iCloud`, `Apple Reminders`).
//!
//! This crate is **pure computation** — no HTTP client, no I/O.
//! Transport implementations are injected via the [`CalDavTransport`]
//! trait.
//!
//! See `docs/architecture.md` §9.5 for the `CalDAV` integration design.
//!
//! ## Modules
//!
//! - [`ical`]      — Minimal iCalendar (RFC 5545) parser and serializer.
//! - [`mapping`]   — Bidirectional field mapping (`Task` ↔ `VTODO`,
//!   `TimeBlock` ↔ `VEVENT`).
//! - [`transport`] — `CalDavTransport` trait for HTTP operations.
//! - [`sync`]      — Sync engine: pull → resolve → push loop.
//! - [`error`]     — Error types.
//!
//! [`CalDavTransport`]: transport::CalDavTransport

pub mod error;
pub mod ical;
pub mod mapping;
pub mod sync;
pub mod transport;

pub use error::Error;
