//! Entry point for the `UniFFI` binding generator.
//!
//! Build with:
//! ```sh
//! cargo build -p tock-uniffi --features cli --bin uniffi-bindgen
//! ```
//!
//! Generate Swift bindings:
//! ```sh
//! cargo run -p tock-uniffi --features cli --bin uniffi-bindgen -- \
//!     generate --library target/debug/libtock_uniffi.dylib \
//!     --language swift \
//!     --out-dir bindings/swift/Sources/TockFFI
//! ```

fn main() {
    uniffi::uniffi_bindgen_main();
}
