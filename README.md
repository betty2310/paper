# Paper

A native, local first desktop app for managing scientific papers, written in Rust with GPUI. Enter a DOI, get the metadata and the open access PDF, organize papers into collections, read and annotate PDFs in a built in viewer, write notes, and copy citations. Runs on macOS, Windows, and Linux.

Status: planning. No application code yet.

## Documents

- [Product requirements](docs/PRD.md): what the MVP does and does not do, and the performance targets.
- [Technology decisions](docs/TECH_STACK.md): the chosen crates and the alternatives that were rejected.
- [Architecture](docs/ARCHITECTURE.md): crate layout, database schema, threading, the PDF viewer pipeline, and GPUI patterns.
- [UI design](docs/UI.md): layout, typography, colors, states, shortcuts, with mockups in `docs/design/`.
- [Roadmap](docs/ROADMAP.md): spikes first, then five milestones.
- Research notes: [GPUI](docs/research/gpui.md), [PDF engines](docs/research/pdf-engines.md), [DOI and PDF sources](docs/research/doi-and-pdf-sources.md).
