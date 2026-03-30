// swift-tools-version:6.1
import PackageDescription

let package = Package(
    name: "QBopomofoTestApp",
    platforms: [
        .macOS(.v14)
    ],
    dependencies: [
        .package(path: "../../base/engine"),
    ],
    targets: [
        .executableTarget(
            name: "QBopomofoTestApp",
            dependencies: [
                .product(name: "Chewing", package: "engine"),
            ],
            path: "Sources"
        ),
    ]
)
