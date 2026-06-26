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
                .product(name: "TockSwift", package: "swift"),
            ],
            path: "Sources/TockApp",
            exclude: [
                "App/TockApp.swift",
            ],
            swiftSettings: [
                .enableExperimentalFeature("StrictConcurrency"),
            ]
        ),
        .testTarget(
            name: "TockAppTests",
            dependencies: ["TockApp"],
            path: "Tests/TockAppTests"
        ),
    ]
)
