# Technology decisions

Each decision lists what we chose, why, and what we rejected. The detailed research behind these choices is in `docs/research/`. Dates and versions are as of 2026-10-28.

## 1. Shape: a Rust engine with a native UI per platform, macOS first

Decision: all logic lives in Rust crates that know nothing about any UI toolkit, exposed through one `paper-engine` crate. Each platform gets its own native UI that calls the engine in process. The first and only shell for 1.0 is the macOS app. Windows and Linux shells come after 1.0 and reuse the engine unchanged.

Why: the author wants the macOS app to be as good as a Mac app can be, which means AppKit windows, native menus, PDFKit, VoiceOver, Dark Mode, system fonts and scrolling, and an app that feels like Preview or Skim rather than like a port. A single cross platform Rust toolkit cannot give that on macOS without compromise, and the engine split makes the later shells cheaper because view models, search, metadata, storage, and citations already exist. The research in `docs/research/native-ui-rust-core.md` has the full comparison.

Rejected: GPUI through gpui-kit (one UI for all platforms, but weekly snapshot crates, slow clean builds, no native menu bar on Windows and Linux, incomplete screen reader support, and a custom look that cannot be pixel perfect on macOS; the research is in `docs/research/gpui.md`). A browser app (the built in PDF viewer has no API, and publisher PDF hosts do not send CORS headers, so downloads need a server). Tauri (one web UI in the system webview, but the author wants native controls). Out of process IPC between engine and UI (crash isolation is not needed for publisher PDFs, and it would cost a shared memory path per platform).

## 2. macOS shell: AppKit with SwiftUI for leaf content

Decision: a Swift app built with Xcode, targeting macOS 13 and later. AppKit owns the window, the custom tab bar in the title bar area, the split view, the collections outline (`NSOutlineView`), the library table (`NSTableView`), the reader (`PDFView`), and the note editor (`NSTextView`). SwiftUI, hosted with `NSHostingView`, draws the detail panel, the annotation cards, the empty states, the toasts, and the settings window.

Why: the mockups specify 36 px and 44 px rows, hairline dividers, bundled serif and sans fonts, and exact paddings. AppKit views give full control of row heights, cell layout, and drawing, and they are the fastest option for a 5,000 row table. SwiftUI on macOS 13 is good for static content with sections and chips, which is what the right hand panels are. PDFKit's `PDFView` is an AppKit view in any case.

Rejected: SwiftUI for the whole app (list and table styling on macOS 13 fights the mockup, and performance of large lists is uneven). Mac Catalyst (an iPad app on the Mac, the opposite of the goal). Raising the minimum to macOS 14 for better SwiftUI (not needed yet; revisit when SwiftUI takes over more of the UI).

## 3. Bridge: UniFFI, in process, static library

Decision: `uniffi 0.32` with proc macros in a `paper-ffi` crate that re-exports the `paper-engine` API as records, enums, objects, async functions, and one foreign trait (`EngineObserver`) that the Swift side implements for events. The engine builds as a `staticlib` for `aarch64-apple-darwin` and `x86_64-apple-darwin`, wrapped into an XCFramework and a Swift package with `cargo-swift` (or a short script around `xcodebuild -create-xcframework`). Callbacks arrive on engine threads, and the Swift side hops to the main thread.

Why: UniFFI is the one bindings generator with years of production use on Apple platforms (Firefox, Element X), its Swift output is official, and proc macros keep the API definition next to the Rust code. A static library avoids a second dylib to sign and keeps the app a single Mach-O plus resources.

Rejected: BoltFFI (faster calls and nicer packaging, but ten months old; revisit when a Windows shell needs C#). swift-bridge (Swift only, would need a second tool later). Hand written C ABI (too much boilerplate for the whole API; still fine for one or two hot functions if a profile shows a need).

## 4. PDF engine on macOS: PDFKit

Decision: the macOS app renders, selects, searches, and navigates PDFs with PDFKit (`PDFDocument`, `PDFPage`, `PDFView`, `PDFSelection`, `PDFOutline`, `PDFAnnotation`). Annotations from the library database are added to the in memory `PDFDocument` as `PDFAnnotation` objects so PDFKit draws them, and the document is never written back to the library file. "Export PDF with annotations" writes a copy with `PDFDocument.write(to:)`. Thumbnails come from `PDFPage.thumbnail(of:for:)`.

Why: PDFKit is the system PDF engine on macOS, the same one Preview uses, and it is what Skim and Highlights are built on. It gives character geometry (`characterBounds(at:)`, `characterIndex(at:)`, `selectionsByLine()`), search, outline, links, annotation reading and writing, and page thumbnails, with no library to ship or sign, no worker thread, and rendering that is already tuned for Retina displays and smooth scrolling. The annotation table stores quads in PDF user space, so nothing in the database depends on PDFKit.

Known costs: PDFKit bugs are Apple's and vary by macOS release, so the reader must be tested on each supported macOS version. `PDFAnnotation.quadrilateralPoints` are relative to the annotation's `bounds` rather than absolute like PDF QuadPoints, so the conversion is one place in the Swift code. Highlight appearance is PDFKit's, so the exact colors in `docs/UI.md` are set through `PDFAnnotation.color` and verified against the mockup.

Rejected for macOS: PDFium through pdfium-render (kept for the later Windows and Linux shells, see `docs/research/pdf-engines.md`, because Windows has no system text or annotation API and poppler on Linux is GPL). MuPDF (AGPL). hayro (read only).

## 5. Annotation storage: database rows, PDF file untouched

Decision: unchanged from the original plan. Annotations live in SQLite keyed by attachment and page with quads in PDF user space (points, origin bottom left, unrotated). The shell draws them through PDFKit on macOS and through an overlay on the later platforms. Export writes real PDF annotations to a copy. Embedded annotations in an imported PDF are read by the shell and offered for import into the database.

Why: Zotero, Skim, sioyek, and ReadCube converged on this model. It keeps the user's file untouched, makes undo and library wide annotation search simple, and keeps the schema engine agnostic so PDFKit and PDFium can both feed it.

## 6. Database: SQLite via rusqlite

Decision: `rusqlite` with the `bundled` feature and FTS5. Numbered SQL migrations applied at startup. The engine owns one connection behind a mutex and runs queries on its own thread pool.

Why: single file, zero setup, fast enough for tens of thousands of items, and the file is the user's data. Synchronous rusqlite fits an engine that owns plain threads.

Rejected: sqlx (async, needs tokio). sled or redb (no SQL, no FTS). GRDB on the Swift side (would split the data layer between two languages and block the later shells).

## 7. HTTP: ureq with rustls

Decision: `ureq 3` (blocking) with `rustls` and `rustls-native-certs`, run on the engine's thread pool. User agent `Paper/<version> (<repo url>; mailto:<contact>)`.

Why: one metadata call and one PDF per added paper is a small volume, and a blocking client on a worker thread is the simplest correct option. rustls with native roots respects corporate or campus proxies on macOS. The engine stays free of an async runtime, which keeps the FFI surface simple.

Rejected: reqwest and tokio (a runtime inside a static library that Swift calls is more moving parts than the workload needs). `URLSession` on the Swift side (would move the lookup pipeline and polite usage rules out of the shared engine).

## 8. Metadata and PDF sources

Decision: unchanged. Crossref REST (polite pool) is the primary metadata source. DOI content negotiation (CSL JSON) is the fallback for DataCite DOIs. OpenAlex supplies open access status and PDF links, with Unpaywall second. arXiv and Europe PMC handle preprints and life science papers. Semantic Scholar is off by default. The app never scrapes aggressively and never bypasses paywalls. See `docs/research/doi-and-pdf-sources.md`.

## 9. Citations: hayagriva and biblatex

Decision: `hayagriva` for formatted citations (about 100 bundled CSL styles including APA) and `biblatex` for BibTeX generation and parsing, both in the engine.

Rejected: `citeproc-rs` (abandoned). Calling doi.org for BibTeX (local generation is instant and works offline).

## 10. Other choices

| Need | Engine (Rust) | macOS shell (Swift) |
|---|---|---|
| Errors | `thiserror` per subsystem, flattened to one `PaperError` enum at the FFI | Swift `Error` from UniFFI, shown as a toast |
| Logging | `tracing` with a subscriber that forwards to the shell | `os.Logger` with subsystem `com.betty2310.paper` |
| Serialization | `serde`, `serde_json` for raw API responses and settings | not needed, UniFFI records |
| IDs | `uuid` v7 | opaque strings |
| Time | `jiff`, UTC RFC 3339 strings in SQLite | `Date` via UniFFI `Timestamp` |
| Paths | `directories` for the settings location | `FileManager` and security scoped bookmarks for the library folder |
| Markdown preview | `pulldown-cmark` to HTML if needed later | `AttributedString(markdown:)` in an `NSTextView` to start |
| Clipboard | none | `NSPasteboard` |
| File dialogs | none | `NSOpenPanel`, `NSSavePanel` |
| Updates | none | Sparkle 2 |
| Fonts | none | Source Serif 4 and Inter bundled, `ATSApplicationFontsPath` in Info.plist |

## 11. Toolchain and build

Rust stable, edition 2024, MSRV tracking uniffi. Xcode 16 or later, Swift 6 language mode on (UniFFI's generated code is mostly `Sendable`; the async foreign trait path has an open issue, so `EngineObserver` is synchronous and the shell dispatches). Release profile with `lto = "thin"`, `codegen-units = 1`, `strip = true`.

Layout: a Cargo workspace at the repository root for the engine, and `apps/macos/` with the Xcode project, the Swift package that wraps the XCFramework, and the generated bindings (checked in, regenerated by a script, diffed in CI). The engine builds in about a minute from scratch; the app builds in Xcode in seconds after that.

CI: GitHub Actions with `cargo test` for the workspace on macos-latest, ubuntu-latest, and windows-latest (the engine must stay portable), and an `xcodebuild test` job on macos-latest that builds the XCFramework, builds the app, runs the XCTest targets, and renders the screenshot fixtures at 1x and 2x.

Packaging: bundle identifier `com.betty2310.paper`, App Sandbox on with a security scoped bookmark for the library folder, Hardened Runtime, `codesign --options runtime --timestamp`, `notarytool`, a DMG, and Sparkle appcast. No dylibs to ship on macOS.

## 12. Later platforms

Windows: WinUI 3 in C# with `uniffi-bindgen-cs`, or `windows-reactor` (Microsoft's WinUI 3 from Rust, preview) if it leaves preview; PDFium through pdfium-render in `paper-pdf` for rendering and text. Linux: GTK 4 with libadwaita in Rust, linking `paper-engine` directly; PDFium as on Windows. Both are after 1.0 and are not on the roadmap until the macOS app has shipped.
