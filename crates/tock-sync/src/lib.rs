//! # tock-sync
//!
//! Event-sourced synchronization for tock: append-only log, conflict
//! resolution with user review, and a transport trait whose
//! implementations live in platform-specific crates (CLI uses `reqwest`,
//! web uses `fetch`, etc.).
//!
//! See `docs/architecture.md` §6 and ADR-003 for the event-sourced sync
//! design.
//!
//! Foundation-phase placeholder.

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(2 + 2, 4);
    }
}
