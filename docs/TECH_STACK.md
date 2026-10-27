# Technology decisions

Each decision lists what we chose, why, and what we rejected. The detailed research behind these choices is in `docs/research/`. Dates and versions are as of 2026-10-27.

## 1. UI framework: GPUI via gpui-kit

Decision: depend on `gpui-kit = "=0.8.1"`, which pins and re-exports `gpui-pre 0.3.10` (Longbridge's weekly snapshot of Zed's GPUI) together with the `gpui-component` widget library.

Why: the official `gpui` crate on crates.io is 0.2.2 from October 2025 and still uses the old Linux renderer. Zed only publishes the core crate and has paused releases, so the current API is only reachable through a git dependency or a third party snapshot. Mixing two GPUI sources in one build fails with duplicate type errors, so every GPUI crate we use must come from one snapshot family. gpui-kit is the only family that ships a widget library (tables with virtual scrolling, trees, tabs, docks, inputs, menus, dialogs, Markdown rendering) built against an exact pin, and it is used in a shipping commercial app (Longbridge Pro). Upgrading becomes a single coordinated version bump.

Rejected: raw git dependency on zed main (weekly breaking changes, no widget library). `gpui-fast` (experimental, can be enabled later as a gpui-kit feature). `gpui-ce` (diverging API). Iced, egui, Slint, Tauri (the user chose GPUI, and GPUI is the only one of these with a text layout and rendering stack proven in a large editor).

Known costs: no native menu bar on Windows and Linux (gpui-kit draws its own). Linux needs a working Vulkan driver. Screen reader support is incomplete. Clean builds are slow. A hello world with `gpui-kit = "=0.8.1"` compiled from scratch in 5 minutes 50 seconds on a 4 core Linux machine (debug profile, dependencies at opt-level 1), with a 4.8 GB target directory. Documentation is API reference only. We should install the gpui-kit agent skills (`npx skills add longbridge/gpui-kit`) so coding sessions have the usage patterns at hand.

## 2. PDF engine: PDFium through pdfium-render

Decision: `pdfium-render 0.9.x` binding to the prebuilt dynamic PDFium library from bblanchon/pdfium-binaries (pinned tag, shipped next to the executable). One dedicated worker thread owns all PDFium calls.

Why: PDFium is Chrome's engine, so it renders essentially every scientific PDF. The crate exposes everything the viewer needs today: page rendering to a bitmap, character bounding boxes and line rectangles for text selection, text search, links, outlines, and reading and creating highlight, underline, strikeout, square, text, and ink annotations. The license is BSD-3 for PDFium and MIT/Apache for the crate, so it works whether the app is open source or not. The prebuilt library is about 8 MB, has no C++ runtime dependency, needs no C toolchain at build time, and is rebuilt weekly for all six OS and architecture pairs.

Rejected: MuPDF (`mupdf-rs`). It is technically the best engine for writing annotations and has incremental save, but it is AGPL, which forces either an AGPL app or a commercial license from Artifex, and it compiles MuPDF from C source with bindgen on every clean build. hayro (pure Rust). Promising and fast, but read only, with no text selection, search, or link API, and the author warns about API churn and memory robustness. Re-evaluate in 2027. Poppler (GPL, needs GLib and cairo on Windows and macOS). Platform native PDFKit and Windows.Data.Pdf (two code paths, Windows has no text or annotation API).

Known costs: PDFium is not thread safe, so rendering, text extraction, and search all go through one worker and a channel. pdfium-render cannot save incrementally, so export writes a new file rather than modifying the original. The macOS dylib must be code signed with the app. The crate's `pdfium_XXXX` feature must match the pinned binary tag.

## 3. Annotation storage: database rows drawn as an overlay

Decision: annotations live in SQLite, keyed by item and page, with quads in PDF user space (points, origin bottom left). The viewer draws them on top of the rendered page. "Export PDF with annotations" writes real PDF annotation objects to a copy. Embedded annotations found in an imported PDF are shown read only and can be imported into the database.

Why: Zotero, Skim, sioyek, and ReadCube all converged on this model. It keeps the user's file untouched, makes undo and search across annotations trivial, produces small row level changes that a future sync can merge, and keeps the PDF engine's weaker write path off the critical path.

Rejected: writing into the PDF on every change (whole file rewrite per edit, conflict prone, risks corrupting the file).

## 4. Database: SQLite via rusqlite

Decision: `rusqlite` with the `bundled` feature (no system SQLite dependency) and FTS5 for full text search over titles, authors, abstracts, notes, and annotation text. Schema migrations are numbered SQL files applied at startup.

Why: single file, zero setup, fast enough for libraries of tens of thousands of items, and the file is the user's data. rusqlite is synchronous, which fits GPUI's background executor model without needing tokio.

Rejected: sqlx (async, needs tokio, compile time query checking is not worth the cost here). sled or redb (no SQL, no FTS).

## 5. HTTP: ureq with rustls

Decision: `ureq 3` (blocking) with `rustls` and `rustls-native-certs`, called from GPUI background tasks. User agent `Paper/<version> (<repo url>; mailto:<contact>)`.

Why: GPUI's executors are not tokio, and the `gpui_tokio` bridge is not published. A blocking client inside `background_spawn` is the simplest correct option, and the download volume is small (one metadata call and one PDF per added paper). rustls with native certificate roots avoids OpenSSL on Linux and respects corporate or campus proxies on Windows and macOS.

Rejected: reqwest (would require vendoring `gpui_tokio` and running a second runtime). Revisit if we need many parallel downloads with cancellation.

## 6. Metadata and PDF sources

Decision: Crossref REST (polite pool) is the primary metadata source. DOI content negotiation (CSL JSON) is the fallback for DataCite DOIs. OpenAlex (free API key, single DOI lookups cost nothing) supplies open access status and PDF links, with Unpaywall as a second source. arXiv and Europe PMC handle preprints and life science papers. Semantic Scholar is off by default because its license restricts commercial use. The app never scrapes publisher pages aggressively and never attempts to bypass paywalls.

See `docs/research/doi-and-pdf-sources.md` for endpoints, rate limits, field mappings, and terms.

## 7. Citations: hayagriva and biblatex

Decision: `hayagriva` (CSL processor from the Typst project, bundles about 100 styles including APA) for formatted citations and `biblatex` for BibTeX generation and parsing.

Rejected: `citeproc-rs` (abandoned). Calling doi.org for BibTeX (works, but local generation is instant and works offline).

## 8. Other crates

| Need | Crate | Note |
|---|---|---|
| Errors | `thiserror` in library crates, `anyhow` in the app | |
| Logging | `tracing`, `tracing-subscriber` | GPUI uses `log`; add `tracing-log` bridge |
| Serialization | `serde`, `serde_json` | raw API responses, settings |
| IDs | `uuid` v7 | time ordered, sync friendly |
| Time | `jiff` or `time` | store UTC RFC 3339 strings in SQLite |
| Paths | `directories` | platform data and config dirs |
| Markdown | gpui-component's Markdown renderer | notes preview |
| Clipboard | GPUI `cx.write_to_clipboard` | |
| File dialogs | GPUI `prompt_for_paths` | native on all platforms |

## 9. Toolchain and build

Rust stable, edition 2024, MSRV tracking gpui-kit. `lld` on Linux and `mold` optional for faster links. In `Cargo.toml`, `[profile.dev.package."*"] opt-level = 1` so dependencies are usable in debug. Release profile with `lto = "thin"`, `codegen-units = 1`, `strip = true`. CI matrix on GitHub Actions for macos-latest, windows-latest, ubuntu-latest building the workspace and running tests, with a headless GPUI window test on Linux.

Packaging: bundle identifier `com.betty2310.paper`. macOS .app with the dylib in `Contents/Frameworks`, signed and notarized. Windows installer with Inno Setup or `cargo-wix`, `pdfium.dll` next to the exe, and an application manifest for DPI awareness. Linux AppImage or tarball with `libpdfium.so` next to the binary (the prebuilt library only needs glibc 2.16).
