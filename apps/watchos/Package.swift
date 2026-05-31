// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "TockWatch",
    platforms: [
        .watchOS(.v10),
    ],
    dependencies: [
        .package(path: "../../bindings/swift"),
    ],
    targets: [
        .target(
            name: "TockWatch",
            dependencies: [
                .product(name: "TockSwift", package: "Tock"),
            ],
            path: "Sources/TockWatch",
            swiftSettings: [
                .enableExperimentalFeature("StrictConcurrency"),
            ]
        ),
    ]
)
