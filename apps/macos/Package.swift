// swift-tools-version: 6.0
// The Paper macOS app. A Swift package for now so it builds and runs with
// `swift run` without an Xcode project; the Xcode project from
// docs/ARCHITECTURE.md replaces this when the app needs a bundle and signing.
import PackageDescription

let package = Package(
    name: "Paper",
    platforms: [.macOS(.v13)],
    dependencies: [
        .package(path: "PaperEngine"),
    ],
    targets: [
        .executableTarget(
            name: "Paper",
            dependencies: [.product(name: "PaperEngine", package: "PaperEngine")],
            path: "Paper"
        ),
    ]
)
