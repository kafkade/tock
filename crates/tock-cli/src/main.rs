//! # tock CLI
//!
//! Command-line interface for tock. Foundation-phase placeholder that
//! prints the version; real subcommands (`add`, `start`, `focus`,
//! `vault`, etc.) land in later issues per `docs/architecture.md` §10.

mod tracing_setup;

fn main() {
    tracing_setup::init_tracing(false);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "tock starting");
    println!(
        "tock {} (foundation-phase scaffold)",
        env!("CARGO_PKG_VERSION")
    );
}
