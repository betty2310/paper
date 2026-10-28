# Paper

A native reference manager for scientific papers. The logic is a Rust engine under `crates/`, and the macOS app under `apps/macos/` is Swift with AppKit, SwiftUI, and PDFKit, calling the engine through UniFFI. Read `docs/PRD.md`, `docs/TECH_STACK.md`, `docs/ARCHITECTURE.md`, and `docs/UI.md` before changing anything.

## UI rules

The macOS app must be elegant and pixel exact. Match `docs/design/library.png` and `docs/design/reader.png` and the tokens in `docs/UI.md`. Paper titles use the bundled serif font (Source Serif 4), everything else uses Inter. Use the 4 px grid, 1 physical pixel borders, one accent color, and the named constants in `Theme.swift`; never put literal sizes or colors in a view. AppKit owns the window, tab bar, sidebar, table, and `PDFView`; SwiftUI is for leaf content hosted in `NSHostingView`. Every interactive element needs hover, pressed, focused, and disabled states. Before finishing any UI change, render the affected states at 1x and 2x with the snapshot tests and inspect the screenshots against the mockups.

## Code rules

Crates under `crates/` must not depend on any UI toolkit, and they must build and pass `cargo test` on macOS, Linux, and Windows. The Swift app talks to the engine only through the `PaperEngine` package; it never opens SQLite, calls the network, or parses API responses itself. Changes to the engine API go in `paper-engine` and `paper-ffi` together, and the generated Swift bindings are regenerated with `scripts/generate-bindings.sh` and committed. `EngineObserver` callbacks arrive on engine threads, so the app hops to the main thread before touching a view. PDFKit calls live only in the reader module, and annotation geometry conversion lives only in `AnnotationGeometry.swift`. The PDF file on disk is never modified; annotations live in SQLite and are only written into a copy on export. Keep raw API responses in `source_records`. Follow the polite usage rules of each metadata API (contact email in the user agent, rate limits).
