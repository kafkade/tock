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
        // Pre-compiled Rust static library (libtock_uniffi.a) packaged as an
        // XCFramework, exposing the C FFI module `tock_uniffiFFI` consumed by
        // the generated Swift bindings. Built by `cargo xtask xcframework`
        // (gitignored — regenerated on demand).
        .binaryTarget(
            name: "TockFFIBinary",
            path: "TockFFI.xcframework"
        ),

        // UniFFI-generated Swift bindings. The single file
        // `Sources/TockFFI/tock_uniffi.swift` is emitted by
        // `cargo xtask xcframework` (gitignored). `Placeholder.swift` keeps
        // the directory tracked before generation has run.
        //
        // Regenerate with:
        //
        //     cargo xtask xcframework
        //
        .target(
            name: "TockFFI",
            dependencies: ["TockFFIBinary"],
            path: "Sources/TockFFI"
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
