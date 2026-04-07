// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "ShuoApp",
    platforms: [
        .macOS(.v13),
    ],
    products: [
        .executable(name: "shuo", targets: ["ShuoApp"]),
        .executable(name: "shuo-bench", targets: ["ShuoBench"]),
    ],
    targets: [
        .executableTarget(
            name: "ShuoApp",
            path: "App/Sources"
        ),
        .executableTarget(
            name: "ShuoBench",
            path: "Bench/Sources"
        ),
    ]
)
