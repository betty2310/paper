# Paper

A native, local first desktop app for managing scientific papers. The logic is a Rust engine, and each platform gets a native app on top of it. Version 1.0 is the macOS app (AppKit, SwiftUI, PDFKit); Windows and Linux follow on the same engine. Enter a DOI, get the metadata and the open access PDF, organize papers into collections, read and annotate PDFs in a built in viewer, write notes, and copy citations.

Status: planning. No application code yet.

## Documents

- [Product requirements](docs/PRD.md): what the MVP does and does not do, and the performance targets.
- [Technology decisions](docs/TECH_STACK.md): the chosen crates and the alternatives that were rejected.
- [Architecture](docs/ARCHITECTURE.md): repository layout, the engine API, database schema, threading, the PDFKit reader, and the macOS app structure.
- [UI design](docs/UI.md): layout, typography, colors, states, shortcuts, with mockups in `docs/design/`.
- [Roadmap](docs/ROADMAP.md): spikes first, then five milestones.
- Research notes: [GPUI](docs/research/gpui.md), [Rust engine with native UI shells](docs/research/native-ui-rust-core.md), [PDF engines](docs/research/pdf-engines.md), [DOI and PDF sources](docs/research/doi-and-pdf-sources.md).
