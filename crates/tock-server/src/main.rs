//! # tock-server
//!
//! Optional sync server for tock. Licensed under **AGPL-3.0-only** — see
//! `crates/tock-server/LICENSE`.
//!
//! The server is an encrypted blob store: it never sees plaintext user
//! data. See `docs/architecture.md` §6 and ADR-006 for the licensing
//! rationale.
//!
//! Foundation-phase placeholder.

fn main() {
    println!(
        "tock-server {} (foundation-phase scaffold, AGPL-3.0-only)",
        env!("CARGO_PKG_VERSION")
    );
}
