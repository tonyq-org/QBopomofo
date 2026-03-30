// swift-tools-version:6.1
import PackageDescription

let package = Package(
    name: "QBopomofo",
    platforms: [
        .macOS(.v13)
    ],
    dependencies: [
        .package(path: "../base/engine"),
    ],
    targets: [
        .executableTarget(
            name: "QBopomofo",
            dependencies: [
                .product(name: "Chewing", package: "engine"),
            ],
            path: "Sources",
            linkerSettings: [
                .linkedFramework("InputMethodKit"),
                .linkedFramework("Cocoa"),
            ]
        ),
    ]
)
