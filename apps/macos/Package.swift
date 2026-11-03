// swift-tools-version: 6.0
// The Paper macOS app. A Swift package for now so it builds and runs with
// `swift run` without an Xcode project; scripts/package-macos-app.sh wraps the
// release build in Paper.app. The Xcode project from docs/ARCHITECTURE.md
// replaces this when the app needs the sandbox and notarization.
import PackageDescription

let package = Package(
    name: "Paper",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(path: "PaperEngine"),
        .package(url: "https://github.com/sparkle-project/Sparkle", from: "2.10.0"),
    ],
    targets: [
        .executableTarget(
            name: "Paper",
            dependencies: [
                .product(name: "PaperEngine", package: "PaperEngine"),
                .product(name: "Sparkle", package: "Sparkle"),
            ],
            path: "Paper",
            exclude: ["Info.plist", "Resources/AppIcon.iconset"],
            resources: [.copy("Resources/AppIcon.icns")],
            // Inside Paper.app the executable is in Contents/MacOS and
            // Sparkle.framework in Contents/Frameworks.
            linkerSettings: [.unsafeFlags(["-Xlinker", "-rpath", "-Xlinker", "@executable_path/../Frameworks"])]
        ),
    ]
)
