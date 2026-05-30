// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "Tock",
    platforms: [
        .iOS(.v17),
        .macOS(.v14),
    ],
    products: [
        .library(
            name: "TockSwift",
            targets: ["TockSwift"]
        ),
    ],
    targets: [
        // UniFFI-generated Swift bindings + C header.
        //
        // After building the Rust library, generate these files with:
        //
        //     cargo run -p tock-uniffi --features cli --bin uniffi-bindgen -- \
        //         generate --library target/debug/libtock_uniffi.dylib \
        //         --language swift \
        //         --out-dir bindings/swift/Sources/TockFFI
        //
        // On macOS the library is `.dylib`, on Linux `.so`, on Windows `.dll`.
        // The generated output includes:
        //   - tock_uniffi.swift   (Swift types + FFI calls)
        //   - tock_uniffiFFI.h    (C header)
        //   - tock_uniffiFFI.modulemap
        //
        // Once an XCFramework is built (see docs), replace this source
        // target with a binaryTarget pointing at the framework.
        .target(
            name: "TockFFI",
            path: "Sources/TockFFI",
            publicHeadersPath: "."
        ),

        // Idiomatic Swift wrapper: async/await, Sendable conformances,
        // and SwiftUI-friendly extensions.
        .target(
            name: "TockSwift",
            dependencies: ["TockFFI"],
            path: "Sources/TockSwift"
        ),

        .testTarget(
            name: "TockSwiftTests",
            dependencies: ["TockSwift"],
            path: "Tests/TockSwiftTests"
        ),
    ]
)
