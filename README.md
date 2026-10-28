# Paper

A native, local first desktop app for managing scientific papers. The logic is a Rust engine, and each platform gets a native app on top of it. Version 1.0 is the macOS app (AppKit, SwiftUI, PDFKit); Windows and Linux follow on the same engine. Enter a DOI, get the metadata and the open access PDF, organize papers into collections, read and annotate PDFs in a built in viewer, write notes, and copy citations.

Status: spike 1 of the [roadmap](docs/ROADMAP.md). A minimal engine and macOS app that talk through UniFFI. The engine keeps 5,000 generated items in memory and fakes the DOI lookup; SQLite and the real metadata sources come in milestone 1.

## Run the demo on macOS

You need Rust (`rustup`) and Xcode 16 or later.

```sh
scripts/build-xcframework.sh          # Rust → apps/macos/PaperEngine/PaperFFI.xcframework
cd apps/macos && swift run Paper      # builds and opens the app
```

Search filters the table on every keystroke through a synchronous engine call. Pasting a DOI such as `10.1038/nature14539` and pressing Add DOI runs an `async` engine call on an engine thread, and the new row appears through the `EngineObserver` callback.

After changing the engine API in `crates/paper-engine` and `crates/paper-ffi`, run `scripts/generate-bindings.sh`, commit the regenerated files in `apps/macos/PaperEngine`, and rebuild the XCFramework. `cargo test --workspace` runs the engine tests on any OS.

## Documents

- [Product requirements](docs/PRD.md): what the MVP does and does not do, and the performance targets.
- [Technology decisions](docs/TECH_STACK.md): the chosen crates and the alternatives that were rejected.
- [Architecture](docs/ARCHITECTURE.md): repository layout, the engine API, database schema, threading, the PDFKit reader, and the macOS app structure.
- [UI design](docs/UI.md): layout, typography, colors, states, shortcuts, with mockups in `docs/design/`.
- [Roadmap](docs/ROADMAP.md): spikes first, then five milestones.
- Research notes: [GPUI](docs/research/gpui.md), [Rust engine with native UI shells](docs/research/native-ui-rust-core.md), [PDF engines](docs/research/pdf-engines.md), [DOI and PDF sources](docs/research/doi-and-pdf-sources.md).
