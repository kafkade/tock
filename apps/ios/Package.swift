// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "TockApp",
    platforms: [
        .iOS(.v17),
        .macOS(.v14),
    ],
    dependencies: [
        .package(path: "../../bindings/swift"),
    ],
    targets: [
        .target(
            name: "TockApp",
            dependencies: [
                .product(name: "TockSwift", package: "Tock"),
            ],
            path: "Sources/TockApp",
            swiftSettings: [
                .enableExperimentalFeature("StrictConcurrency"),
            ]
        ),
    ]
)
