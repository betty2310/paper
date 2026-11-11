# Roadmap

Each milestone ends with something the author can run on their Mac. Risky parts are proven first with small spikes, so the order below is by risk, not by user value. Version 1.0 is the macOS app only. Windows and Linux apps start after 1.0 on the same engine.

## Milestone 0: spikes (about one week)

1. Engine to Swift round trip. A `paper-engine` crate with `open_library`, `list_items`, and one async method, exported through `paper-ffi` with UniFFI, built into an XCFramework by `scripts/build-xcframework.sh`, and called from a minimal Swift app that lists 5,000 generated items in an `NSTableView`. Measure the engine's clean build time, the latency of a synchronous call from Swift, the app's binary size, and the time to regenerate bindings. CI builds and tests the engine on macOS, Linux, and Windows, and builds the app on macOS.
2. PDFKit reader. A `PDFView` in continuous mode showing a 30 page paper. Measure time to the first visible page against the 300 ms target, and resident memory with three papers open at 100% and 200% zoom against the 400 MB target. Confirm on rotated and odd crop box fixtures that PDFKit page space matches the database's PDF user space definition, and record the conversion rule. Build the custom thumbnail strip from `thumbnail(of:for:)` and check scrolling stays smooth.
3. Selection to highlight. Drag select text, group it with `selectionsByLine()`, send the quads to the engine, draw the result as a `PDFAnnotation` in Paper's yellow, then export a copy and open it in Preview, Acrobat, and Chrome. Read the exported annotation back with PDFKit and with `pdfium-render` on Linux CI and check both return the same quads.
4. DOI lookup. A command line tool in `paper-fetch` that takes a DOI and prints the mapped metadata and the PDF URL it would download, tested on 20 DOIs including an arXiv DOI and a paywalled Elsevier paper.
5. Pixel check. One AppKit window with the tab bar, the sidebar, and the library table built to `docs/UI.md` with the bundled fonts, screenshotted at 1x and 2x and laid over `docs/design/library.png`. The result decides whether the table and sidebar stay in AppKit or whether any part can move to SwiftUI.

## Milestone 1: library

Workspace skeleton, SQLite schema with migrations, `Library` service, the `paper-engine` API for items and collections, the Swift package, `LibraryStore`, main window with sidebar and table, sorting and filtering, add by DOI with metadata only, SwiftUI detail panel, delete and undo, main menu with key equivalents. No PDFs yet.

## Milestone 2: PDFs

Open access PDF fetching with progress, drag and drop import with DOI detection, attachment storage, security scoped bookmark for the library folder, reader tab with `PDFView`, zoom, outline, links, in-document search, thumbnail strip, remembering the last page.

## Milestone 3: annotations and notes

Highlight, underline, strikeout, sticky note, rectangle, colors, the floating markup toolbar, the annotations panel, the Markdown note panel, export PDF with annotations, import of embedded annotations.

## Milestone 4: citations and packaging

Copy BibTeX, copy APA, export .bib, citation keys, settings window (contact email, OpenAlex key), library chooser and recent libraries, App Sandbox and Hardened Runtime, signed and notarized DMG, Sparkle updates, crash reports through Sentry when the user agrees, error reporting to a local log, snapshot tests at 1x and 2x for every screen state.

## Milestone 5: Zotero import and 1.0

Import from `zotero.sqlite` and the `storage/` folder: items with DOIs, creators, collections, tags, notes, PDF attachments, and reader annotations (Zotero stores them in `itemAnnotations` with page index and position JSON, which maps onto our quads). Items without a DOI are imported with their fields as given. The import runs from the File menu, shows progress, and reports what was skipped.

## After 1.0

Windows app (WinUI 3, PDFium through `paper-pdf`) and Linux app (GTK 4, PDFium through `paper-pdf`), in the order the author needs them. Then more CSL styles and a style picker, full text search inside PDFs across the library, sync through a user chosen folder, browser extension, group libraries, and ink annotations for tablets.
