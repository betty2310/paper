// swift-tools-version: 6.0
// The only way the app reaches the Rust engine. Run scripts/build-xcframework.sh
// once to produce PaperFFI.xcframework before building.
import PackageDescription

let package = Package(
    name: "PaperEngine",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "PaperEngine", targets: ["PaperEngine"]),
    ],
    targets: [
        // The Rust static library with its C header (module `paper_ffiFFI`).
        .binaryTarget(name: "PaperFFI", path: "PaperFFI.xcframework"),
        // The generated Swift API on top of it.
        .target(name: "PaperEngine", dependencies: ["PaperFFI"]),
    ]
)
