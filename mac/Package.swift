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
            swiftSettings: [
                // Newer SDKs mark IMKInputController @MainActor; under the
                // Swift 6 language mode our overrides fail isolation checks
                // (this is what broke every CI release build). Pin to the
                // Swift 5 mode until the controller is properly annotated.
                .swiftLanguageMode(.v5),
            ],
            linkerSettings: [
                .linkedFramework("InputMethodKit"),
                .linkedFramework("Cocoa"),
            ]
        ),
    ]
)
