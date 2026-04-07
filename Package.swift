// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "HJVoiceBridge",
    platforms: [
        .macOS(.v13),
    ],
    products: [
        .executable(name: "hj-voice", targets: ["HJVoiceBridge"]),
    ],
    targets: [
        .executableTarget(
            name: "HJVoiceBridge",
            path: "App/Sources"
        ),
    ]
)
