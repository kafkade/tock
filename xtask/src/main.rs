//! # xtask
//!
//! Internal `cargo xtask` runner for build orchestration. See
//! `docs/architecture.md` §4.4.
//!
//! ## Subcommands
//!
//! * `xcframework` — regenerate the `UniFFI` Swift bindings and build the
//!   `TockFFI.xcframework` consumed by `bindings/swift`. See issue #119 and
//!   ADR-005.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Apple targets whose static-lib slices are fused into the macOS slice.
const MACOS_TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
/// iOS device target (single arch).
const IOS_DEVICE_TARGET: &str = "aarch64-apple-ios";
/// iOS simulator target (Apple-silicon hosts).
const IOS_SIM_TARGET: &str = "aarch64-apple-ios-sim";
/// The crate built into the FFI static library.
const FFI_CRATE: &str = "tock-uniffi";
/// Base name of the produced static library (`lib<name>.a`).
const FFI_LIB_STEM: &str = "tock_uniffi";
/// Cargo profile used for the FFI build. A dedicated profile (rather than
/// `release`) keeps symbols: the workspace `release` profile sets
/// `strip = "symbols"`, which would remove the `#[no_mangle]` `UniFFI`
/// scaffolding the generated Swift links against.
const FFI_PROFILE: &str = "apple-ffi";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help" | "-h" | "--help") => {
            print_help();
        }
        Some("xcframework") => {
            if let Err(err) = run_xcframework() {
                eprintln!("xtask: xcframework failed: {err}");
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
         SUBCOMMANDS:\n  \
           help           Show this message\n  \
           xcframework    Regenerate UniFFI Swift bindings + build TockFFI.xcframework\n\n\
         Future subcommands (per docs/architecture.md §4.4):\n  \
         check-purity, cli-snapshot, wasm-build"
    );
}

/// Regenerate the Swift bindings and assemble `TockFFI.xcframework`.
///
/// Pipeline (all paths relative to the workspace root):
/// 1. `rustup target add` the four Apple targets (idempotent).
/// 2. `cargo build --profile apple-ffi` the FFI crate for each Apple target.
/// 3. Run `uniffi-bindgen` against the macOS dylib to emit Swift + header
///    + modulemap, copying the Swift into `bindings/swift/Sources/TockFFI/`.
/// 4. `lipo` the two macOS static libs into one universal slice.
/// 5. `xcodebuild -create-xcframework` over the macOS, iOS-device, and
///    iOS-simulator slices, sharing one headers directory.
fn run_xcframework() -> Result<(), String> {
    let root = workspace_root()?;
    let scratch = root.join("target").join("uniffi-xcframework");
    recreate_dir(&scratch)?;

    build_apple_targets(&root)?;
    let gen_dir = generate_bindings(&root, &scratch)?;
    install_swift_source(&root, &gen_dir)?;
    let headers = prepare_headers(&scratch, &gen_dir)?;
    let macos_lib = lipo_macos(&root, &scratch)?;
    create_xcframework(&root, &headers, &macos_lib)
}

/// All Apple targets the FFI crate is compiled for.
const fn apple_targets() -> [&'static str; 4] {
    [
        MACOS_TARGETS[0],
        MACOS_TARGETS[1],
        IOS_DEVICE_TARGET,
        IOS_SIM_TARGET,
    ]
}

/// Install the Apple Rust targets and build the FFI crate for each.
fn build_apple_targets(root: &Path) -> Result<(), String> {
    println!("==> Ensuring Apple Rust targets are installed");
    for target in apple_targets() {
        run(
            Command::new("rustup").args(["target", "add", target]),
            &format!("rustup target add {target}"),
        )?;
    }

    println!("==> Building {FFI_CRATE} for Apple targets ({FFI_PROFILE})");
    for target in apple_targets() {
        run(
            Command::new("cargo").current_dir(root).args([
                "build",
                "--profile",
                FFI_PROFILE,
                "-p",
                FFI_CRATE,
                "--target",
                target,
            ]),
            &format!("cargo build --target {target}"),
        )?;
    }
    Ok(())
}

/// Run `uniffi-bindgen` against the macOS dylib, returning the output dir.
fn generate_bindings(root: &Path, scratch: &Path) -> Result<PathBuf, String> {
    println!("==> Generating Swift bindings");
    let bindgen_lib = ffi_artifact(root, MACOS_TARGETS[0], &format!("lib{FFI_LIB_STEM}.dylib"));
    let gen_dir = scratch.join("generated");
    recreate_dir(&gen_dir)?;
    run(
        Command::new("cargo")
            .current_dir(root)
            .args([
                "run",
                "--release",
                "-p",
                FFI_CRATE,
                "--features",
                "cli",
                "--bin",
                "uniffi-bindgen",
                "--",
                "generate",
                "--library",
            ])
            .arg(&bindgen_lib)
            .args(["--language", "swift", "--out-dir"])
            .arg(&gen_dir),
        "uniffi-bindgen generate",
    )?;
    Ok(gen_dir)
}

/// Copy the generated Swift source into the SPM `TockFFI` target.
fn install_swift_source(root: &Path, gen_dir: &Path) -> Result<(), String> {
    let swift_dst = root
        .join("bindings")
        .join("swift")
        .join("Sources")
        .join("TockFFI")
        .join(format!("{FFI_LIB_STEM}.swift"));
    copy_file(&gen_dir.join(format!("{FFI_LIB_STEM}.swift")), &swift_dst)?;
    println!("    wrote {}", swift_dst.display());
    Ok(())
}

/// Build the shared headers directory (C header + `module.modulemap`).
fn prepare_headers(scratch: &Path, gen_dir: &Path) -> Result<PathBuf, String> {
    let headers = scratch.join("Headers");
    recreate_dir(&headers)?;
    copy_file(
        &gen_dir.join(format!("{FFI_LIB_STEM}FFI.h")),
        &headers.join(format!("{FFI_LIB_STEM}FFI.h")),
    )?;
    copy_file(
        &gen_dir.join(format!("{FFI_LIB_STEM}FFI.modulemap")),
        &headers.join("module.modulemap"),
    )?;
    Ok(headers)
}

/// Fuse the two macOS arch static libs into one universal slice.
///
/// The library file inside each xcframework slice must be `lib`-prefixed for
/// `SwiftPM` to accept it, so the universal lib lives in its own directory under
/// the canonical `lib<stem>.a` name.
fn lipo_macos(root: &Path, scratch: &Path) -> Result<PathBuf, String> {
    println!("==> Fusing macOS slice with lipo");
    let macos_dir = scratch.join("macos");
    recreate_dir(&macos_dir)?;
    let macos_lib = macos_dir.join(format!("lib{FFI_LIB_STEM}.a"));
    let mut lipo = Command::new("lipo");
    lipo.arg("-create");
    for target in MACOS_TARGETS {
        lipo.arg(ffi_artifact(root, target, &format!("lib{FFI_LIB_STEM}.a")));
    }
    lipo.arg("-output").arg(&macos_lib);
    run(&mut lipo, "lipo -create macOS")?;
    Ok(macos_lib)
}

/// Run `xcodebuild -create-xcframework` over all three slices.
fn create_xcframework(root: &Path, headers: &Path, macos_lib: &Path) -> Result<(), String> {
    println!("==> Creating TockFFI.xcframework");
    let xcframework = root
        .join("bindings")
        .join("swift")
        .join("TockFFI.xcframework");
    if xcframework.exists() {
        std::fs::remove_dir_all(&xcframework)
            .map_err(|e| format!("remove {}: {e}", xcframework.display()))?;
    }
    let ios_device_lib = ffi_artifact(root, IOS_DEVICE_TARGET, &format!("lib{FFI_LIB_STEM}.a"));
    let ios_sim_lib = ffi_artifact(root, IOS_SIM_TARGET, &format!("lib{FFI_LIB_STEM}.a"));
    let mut xcb = Command::new("xcodebuild");
    xcb.arg("-create-xcframework");
    for lib in [macos_lib, &ios_device_lib, &ios_sim_lib] {
        xcb.arg("-library").arg(lib).arg("-headers").arg(headers);
    }
    xcb.arg("-output").arg(&xcframework);
    run(&mut xcb, "xcodebuild -create-xcframework")?;

    println!("✔ TockFFI.xcframework ready at {}", xcframework.display());
    Ok(())
}

/// Path to an `apple-ffi`-profile build artifact for `target`.
fn ffi_artifact(root: &Path, target: &str, file: &str) -> PathBuf {
    root.join("target")
        .join(target)
        .join(FFI_PROFILE)
        .join(file)
}

/// Resolve the workspace root from this crate's compile-time manifest dir.
fn workspace_root() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "could not resolve workspace root from CARGO_MANIFEST_DIR".to_string())
}

/// Remove `dir` if present and recreate it empty.
fn recreate_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("remove {}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))
}

/// Copy `src` to `dst`, creating parent directories as needed.
fn copy_file(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| format!("copy {} -> {}: {e}", src.display(), dst.display()))
}

/// Run `cmd`, returning an error if it cannot start or exits non-zero.
fn run(cmd: &mut Command, desc: &str) -> Result<(), String> {
    let status = cmd
        .status()
        .map_err(|e| format!("failed to spawn `{desc}`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{desc}` exited with {status}"))
    }
}
