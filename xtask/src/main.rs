//! # xtask
//!
//! Internal `cargo xtask` runner for build orchestration. Subcommands
//! land in future issues — see `docs/architecture.md` §4.4. Foundation
//! scaffold simply prints help.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help" | "-h" | "--help") => print_help(),
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
         SUBCOMMANDS:\n    help    Show this message\n\n\
         Future subcommands (per docs/architecture.md §4.4):\n  \
         check-purity, cli-snapshot, wasm-build, xcframework"
    );
}
