# Research: Rust core engine with a native UI per platform

Verified 2026-10-28 against crates.io, GitHub, the UniFFI and BoltFFI docs, Microsoft Learn, and Apple's PDFKit docs. This note answers one question. Instead of one GPUI app, should Paper be a Rust engine with a separate native UI on each OS (SwiftUI or AppKit on macOS, WinUI 3 on Windows, GTK 4 on Linux), with the UI talking to the engine through an FFI bridge or IPC?

Decision (2026-10-28): the author chose this model. Version 1.0 is the macOS app with AppKit, SwiftUI for leaf content, and PDFKit for the reader, bridged to the engine with UniFFI. Windows and Linux apps come after 1.0 with PDFium. The decisions are recorded in `docs/TECH_STACK.md` and the rest of this note is the research that led to them, including the recommendation below to keep PDFium on every platform, which was not followed for macOS because PDFKit covers the whole reader there and removes a library to ship and sign.

The short answer is that the engine half is cheap, because `docs/ARCHITECTURE.md` already keeps everything except `paper-app` free of GPUI. The expensive half is writing and maintaining three UIs in three languages, and the hardest piece of each UI is the PDF canvas. The sections below give the options, the numbers, the precedents, and a recommendation.

## What already fits

The planned workspace has `paper-core`, `paper-ids`, `paper-fetch`, `paper-pdf`, and `paper-cite` with no GPUI dependency, and one `paper-app` crate that holds the views. Under the engine model, `paper-app` is replaced by one engine crate and one shell per platform.

```text
crates/
├── paper-core, paper-ids, paper-fetch, paper-pdf, paper-cite   unchanged
├── paper-engine/     one public API: commands, view models, events, threads
├── paper-ffi/        generated bindings (UniFFI or BoltFFI) plus a few raw C functions
shells/
├── macos/            Swift package + Xcode app (SwiftUI with AppKit views where needed)
├── windows/          C# WinUI 3 app, or Rust windows-reactor app
└── linux/            Rust GTK 4 + libadwaita app, links paper-engine directly
```

```diagram
┌───────────────┐  ┌────────────────┐  ┌───────────────┐
│ macOS shell   │  │ Windows shell  │  │ Linux shell   │
│ Swift         │  │ C# or Rust     │  │ Rust (GTK 4)  │
└───────┬───────┘  └───────┬────────┘  └───────┬───────┘
        │ generated        │ generated         │ plain Rust call
        ▼                  ▼                   ▼
┌─────────────────────────────────────────────────────────┐
│ paper-engine: Engine, commands, view models, observer   │
│ owns the PDF worker thread, SQLite, HTTP, thread pool   │
└─────────────────────────────────────────────────────────┘
```

The engine API is the design work that pays off under either decision. Element X keeps its shells thin by putting view model logic (room list, timeline) in a `matrix-sdk-ui` crate rather than in Swift and Kotlin, and Paper should do the same with `paper-engine` (item rows, detail panel data, annotation cards, selection geometry, search results).

## How the UI talks to the engine

There are four ways to connect a UI to the engine. They differ mostly in how the page bitmaps cross the boundary, because everything else in Paper is small data. A Letter page at 2x device scale and 100% zoom is 7.76 MB of BGRA (`docs/research/pdf-engines.md`), and the viewer asks for several per second while scrolling.

| Option | Call cost | Page bitmap | Crash isolation | Languages | Build and tooling |
|---|---|---|---|---|---|
| In process, generated bindings (UniFFI, BoltFFI) | about 1 µs per call with UniFFI, under 1 µs with BoltFFI for primitives (BoltFFI's own benchmark) | one copy through the binding's buffer, or zero copy with a raw pointer function | none, a PDFium crash kills the app | Swift, Kotlin, C#, Python; Go and Java with third party or BoltFFI | one `cdylib` per target, generated sources checked in or built in CI |
| In process, hand written C ABI (`cbindgen 0.29.4`) | sub µs | zero copy by design | none | any language with C interop | most boilerplate, every type crossed by hand |
| In process, same language (Rust GTK shell) | a function call | zero copy | none | Rust only | nothing extra |
| Out of process IPC (JSON-RPC over stdio, gRPC with `tonic 0.14` over a unix socket or named pipe) | tens to hundreds of µs per round trip plus serialization | needs shared memory (memfd on Linux, `shm_open` on macOS, `CreateFileMapping` on Windows) or an 8 MB copy per page through the socket | the UI survives an engine crash and can restart it | any | two executables, protocol versioning, process lifetime, shared memory code per OS |

IPC is the model Mullvad uses. The `mullvad-daemon` Rust program exposes a gRPC management interface on `/var/run/mullvad-vpn` (macOS and Linux) or `//./pipe/Mullvad VPN` (Windows), and the desktop GUI, the CLI, and the Android app are all clients. Mullvad needs a daemon anyway because the tunnel runs as a system service without a logged in user. Paper has no such requirement, and the one real benefit of IPC for Paper, isolating PDFium from the UI, costs a shared memory implementation on three platforms. Chrome does isolate PDFium in a sandboxed process, but Paper's PDFs come from publishers and arXiv rather than from untrusted web pages, and `paper-pdf` already talks to its worker through a request channel, so the worker can be moved into a child process later without changing the engine API.

The in process bridge is the right default. For the bitmaps, keep the generated API for everything else and add two or three raw `extern "C"` functions, for example `paper_tile_lock(handle) -> (ptr, len, stride)` and `paper_tile_release(handle)`, so each shell wraps the buffer without copying. On macOS a `CGDataProvider` can wrap the pointer directly, on Linux `gdk::MemoryTexture` takes a `glib::Bytes` that can own the Rust allocation, and on Windows Win2D's `CanvasBitmap.CreateFromBytes` or a `WriteableBitmap` copies once (about 1 ms for 8 MB). Even the copying path is far inside the 16 ms frame budget.

## Binding generators

| Tool | Version | Languages | Notes |
|---|---|---|---|
| UniFFI (Mozilla) | 0.32.2, 2026-09-23 | Swift, Kotlin, Python, Ruby official; C# and Go third party | Proc macro or UDL definitions, async functions, objects, records, enums, errors, and "foreign traits" for callbacks from Rust into the shell. `bytes` becomes `Data` with a copy. Used in production by Firefox (application-services) and by Element X through `matrix-rust-sdk` (Swift package `matrix-rust-components-swift` 26.10.15). Swift 6 strict concurrency still has rough edges, e.g. issue #2929 (async foreign traits fail to compile in Swift 6 language mode, June 2026). The README says it is production ready but "a long way from a 1.0 release". |
| uniffi-bindgen-cs (NordSecurity) | v0.11.1 targets UniFFI 0.31.0 | C# | Separate project, MPL-2.0, 187 stars, 25 open issues, MSRV 1.88. It lags UniFFI by one minor version, so a C# Windows shell pins the whole workspace to the UniFFI version the C# generator supports. |
| BoltFFI | 0.32.0 on crates.io, first release 2025-12, MIT, 906 stars, active | Swift, Kotlin, Java, C#, TypeScript (WASM), Python; C experimental | Zero copy for primitives and plain structs, async, streams, callbacks. `boltffi pack apple` builds an XCFramework and `pack csharp` builds a NuGet package. Crux migrated all its templates from UniFFI to BoltFFI in 2026. It is ten months old and has one maintainer group, so API churn is likely. |
| swift-bridge | 0.1.59, 2026-01 | Swift only | Good Swift ergonomics, no help for Windows. |
| cbindgen | 0.29.4 | C header | Use for the bitmap functions only. |
| cargo-swift | 0.11.1, 2026-05 | packaging | Builds the XCFramework and Swift package from a UniFFI crate in one command. |

Recommendation: UniFFI with proc macros for the API, because it is the one generator with years of production use behind it and official Swift support. Keep the API narrow and typed by records and enums so that switching to BoltFFI later is a mechanical change, and run BoltFFI in the first spike as a comparison, since its packaging (`pack apple`, `pack csharp`) and its C# support are first party while UniFFI's C# path is third party and lags.

## Native UI toolkits per platform

### macOS

SwiftUI with AppKit views where SwiftUI falls short (the PDF canvas, the thumbnail strip, and the item table if SwiftUI `Table` is too slow at 5,000 rows). Requirements: Xcode, a Swift package that wraps the Rust XCFramework, code signing of the Rust dylib and `libpdfium.dylib` with the app, and Sparkle or the App Store for updates. The shell gets the native menu bar, Services, Quick Look, VoiceOver, text input, Dark Mode, and scroll physics for free, and bundled Source Serif 4 and Inter work as app fonts. GPUI is most mature on macOS, so the gain on macOS is smaller than on the other two platforms.

### Windows

Microsoft's recommended path for new desktop apps is WinUI 3 in C# on the Windows App SDK (stable 2.5.1 as of 2026-09-30). Unpackaged apps must ship the Windows App SDK runtime installer or use self contained deployment, which makes the installer larger. There is now a Rust option. `windows-reactor` 0.100.0 (Microsoft, April 2026, "currently a preview") is a declarative component library over native WinUI 3 controls with typed messages, background work, multiple windows, and a `Canvas` plus Composition sample. It would let the Windows shell stay in Rust and skip the C# binding generator, at the cost of depending on a preview. WPF remains a stable fallback with full custom drawing and no runtime installer, but it looks older. Neither WinUI 3 nor WPF ships a PDF view with text or annotation support.

### Linux

GTK 4 (`gtk4 0.11.5`) with libadwaita (`0.9.2`) or relm4 (`0.11.0`) in Rust, linking `paper-engine` directly with no bridge at all. GNOME Papers, the GNOME document viewer, is a GTK 4 + libadwaita + Rust app, so a Rust PDF reader in GTK is proven. Custom widgets draw page tiles with `gdk::MemoryTexture` in `snapshot`. The costs are that GTK apps look like GNOME apps (the mockup's look needs custom CSS, and KDE users get a GTK app), and that GTK 4 needs the GTK 4 and libadwaita development packages at build time and the runtime libraries or a Flatpak at install time. There is no Vulkan requirement, which removes GPUI's biggest Linux support risk.

## The PDF viewer is the hard part

The engine model forces a choice about who renders and lays out pages. Using each platform's PDF stack does not work across all three platforms.

| Platform stack | Render | Text and selection | Search | Annotations | License |
|---|---|---|---|---|---|
| PDFKit (macOS) `PDFView`, `PDFSelection`, `PDFAnnotation` | yes | yes | yes | yes, writes to the PDF model | system |
| Windows.Data.Pdf (Windows) | yes, `PdfPage.RenderToStreamAsync` | no | no | no | system |
| WebView2 PDF viewer (Windows) | yes | inside the viewer only | inside the viewer only | no API | system |
| poppler-glib (Linux) | yes | yes | yes | yes | GPL-2 |

Windows has no text geometry at all, so a Windows shell needs PDFium regardless. Using PDFKit on macOS and poppler on Linux would give three engines with three different character box behaviors feeding one annotation table, and PDFKit has a history of regressions between macOS releases. The workable design keeps one PDFium engine in `paper-pdf` on all platforms, exactly as planned, and makes each shell a thin tile painter. The engine computes page sizes, tiles, character boxes, line grouping, hit testing, selection quads, and annotation geometry in PDF user space, and the shell converts points to pixels, draws tiles and overlay quads, and reports mouse and keyboard events back. Each shell still needs its own scroll container with variable page heights, tile painting, overlay painting, selection drag handling, and the floating markup toolbar. A rough estimate is 1,500 to 3,000 lines per platform for the reader surface alone, on top of the library table, sidebar tree, detail panel, note editor, dialogs, menus, and settings.

## Precedents

| Product | Core | Shells | Bridge | Relevance |
|---|---|---|---|---|
| Element X (Element) | `matrix-rust-sdk` with `matrix-sdk-ui` view models | SwiftUI (iOS), Jetpack Compose (Android) | UniFFI | The closest match to the proposed shape, but mobile only. The view model crate is the pattern to copy. |
| 1Password 8 | Rust core with everything except UI | planned SwiftUI Mac app plus a web UI shell | custom, `typeshare` for types | The shipped Mac app uses the web UI shell. A large team still chose one UI stack over per platform native. |
| Mullvad VPN | Rust daemon | Electron desktop, native Android, separate Swift iOS app | gRPC over unix socket or named pipe | The IPC precedent. The daemon exists for the system service, not for UI reasons. |
| Firefox application-services | Rust components | Firefox iOS and Android | UniFFI (its origin) | Shows UniFFI at scale. |
| GNOME Papers | poppler | GTK 4 + libadwaita in Rust | none | Rust GTK PDF reader. |
| Crux (Red Badger) | pure Rust core, effects run by the shell | Swift, Kotlin, TypeScript | BoltFFI since 2026 | A framework for this exact split. Its pure core design would push SQLite, HTTP, and PDFium out to each shell, which is backwards for Paper, so it is not recommended. |
| Cap (screen recorder) | Rust | GPUI, rebuilt from Tauri in 2026 | none | The counterexample. One Rust UI for all platforms cut CPU from 76.6% to 17.5% and memory from 3.7 GB to 440 MB (`docs/research/gpui.md`). |
| jffi | Rust | SwiftUI, Compose, GTK 4 (Python) | UniFFI | A small CLI that scaffolds this architecture. Too small to depend on, but it shows the shape is common. |

## Comparison with the GPUI plan

| | One GPUI app (current plan) | Rust engine plus native shells |
|---|---|---|
| UI code | one implementation | three implementations, feature parity by hand |
| Languages | Rust | Rust plus Swift, plus C# unless `windows-reactor` is used |
| Clean build | 5 min 50 s for a hello world, 4.8 GB target dir | engine only, roughly a minute; each shell builds with its platform tools |
| Look | pixel exact match to the mockups on all platforms, one theme module | each toolkit has its own controls and defaults, so "pixel exact and the same everywhere" means fighting three toolkits, or the mockups become per platform |
| Native menus, dialogs, IME, screen readers, Dark Mode, scroll physics | macOS good; Windows and Linux have no native menu bar and incomplete screen reader support | all free on every platform |
| Linux | needs Vulkan | needs GTK 4 runtime |
| Ecosystem risk | weekly `gpui-pre` snapshots, one vendor, no official release | platform SDKs are stable; UniFFI is pre 1.0 but widely used; BoltFFI and `windows-reactor` are young |
| Binary size | 15 to 40 MB | engine dylib plus PDFium about 10 to 15 MB, shells small, Windows needs the App SDK runtime |
| Testing | headless GPUI window tests | engine tests in plain Rust; each shell tested with its platform tools (XCTest, WinAppDriver, GTK test) |
| Solo author effort | one UI to build and keep current | roughly three times the UI milestones (the UI parts of milestones 1 to 4), with the second and third shells cheaper because the engine and view models exist |

## Recommendation

1. Build `paper-engine` as the one public API of the core regardless of the UI decision. It costs little now, makes the GPUI shell thinner, and keeps the native shell option open. The API shape: an `Engine` object opened on a library folder; commands (`add_by_doi`, `import_pdf`, `search`, `open_document`, `render_tile`, `select_text`, `create_annotation`, `copy_bibtex`); view model records (`ItemRow`, `ItemDetail`, `CollectionNode`, `AnnotationCard`, `PageLayout`); and an observer trait the shell implements for events (`ItemsChanged`, `LookupProgress`, `TileReady`). The engine owns its threads, and callbacks arrive on engine threads, so each shell hops to its main thread (`DispatchQueue.main`, `DispatcherQueue`, `glib::MainContext`).
2. If the native shell model is chosen, use an in process bridge, not IPC. Generate the API with UniFFI, add raw C functions for zero copy tiles, keep PDFium as the single engine, and treat each shell as a tile painter plus native controls. Use GTK 4 in Rust on Linux with no bridge, SwiftUI with AppKit on macOS, and decide between C# WinUI 3 (stable, needs the C# generator) and `windows-reactor` (Rust, preview) for Windows in a spike.
3. Ship one platform first, the one the author uses every day, and add the other shells after the engine API has settled through real use. A three platform MVP with three native shells is more than one person can finish on the current roadmap.
4. A hybrid is possible because the engine is GPUI free. For example, a SwiftUI shell on macOS and the GPUI shell on Windows and Linux, or GPUI everywhere now and a native macOS shell later. The engine API makes either order possible.

## Open questions for the author

1. Which OS is the daily machine? It decides the first shell and whether the macOS gain (small, GPUI is good there) or the Windows and Linux gain (large, native menus and accessibility) applies.
2. Is the goal the same custom look on all platforms, as `docs/UI.md` specifies, or a platform native look on each? Native shells argue for per platform looks, which changes the mockups.
3. Are three UI codebases in up to three languages acceptable for a single author, given that the MVP success measure is one month of daily use?

## Spike to settle it

Before deciding, build one spike in about a week. Write `paper-engine` with `open_library`, `list_items`, `open_document`, and `render_tile`, expose it with UniFFI, and build a SwiftUI (or GTK) window that lists 5,000 items and scrolls a 30 page PDF at 100% and 200% zoom. Measure the bitmap path (copy versus pointer), first page time against the 300 ms target, memory against the 400 MB target, and the number of lines in the shell. Compare with the GPUI spike in `docs/ROADMAP.md` milestone 0, item 2, on the same machine.
