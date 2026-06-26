//! # xtask
//!
//! Internal `cargo xtask` runner for build orchestration. Subcommands
//! land in future issues — see `docs/architecture.md` §4.4.

mod i18n_check;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help" | "-h" | "--help") => print_help(),
        Some("i18n-check") => {
            if let Err(report) = i18n_check::run() {
                eprintln!("i18n-check: FAILED\n{report}");
                std::process::exit(1);
            }
        }
        Some(other) => {
            eprintln!("xtask: unknown subcommand '{other}'");
            print_help();
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!(
        "xtask — internal build orchestration\n\n\
         USAGE:\n    cargo xtask <SUBCOMMAND>\n\n\
         SUBCOMMANDS:\n    \
         help         Show this message\n    \
         i18n-check   Validate localization catalogs (id parity + parse)\n\n\
         Future subcommands (per docs/architecture.md §4.4):\n  \
         check-purity, cli-snapshot, wasm-build, xcframework"
    );
}
