// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "TockMac",
    platforms: [
        .macOS(.v14),
    ],
    dependencies: [
        .package(path: "../../bindings/swift"),
    ],
    targets: [
        .executableTarget(
            name: "TockMac",
            dependencies: [
                .product(name: "TockSwift", package: "Tock"),
            ],
            path: "Sources/TockMac",
            swiftSettings: [
                .enableExperimentalFeature("StrictConcurrency"),
            ]
        ),
    ]
)
