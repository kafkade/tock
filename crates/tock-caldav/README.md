# tock-caldav

CalDAV bidirectional sync for tock. Maps tasks to VTODO and time blocks
to VEVENT, syncs with CalDAV servers (Nextcloud, Radicale, iCloud,
Apple Reminders).

This crate is **pure computation** — no HTTP client, no I/O. Transport
implementations are injected via the `CalDavTransport` trait.

See `docs/architecture.md` §9.5 for the CalDAV integration design.
