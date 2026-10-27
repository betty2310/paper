# Paper

A native reference manager for scientific papers, written in Rust with GPUI (through gpui-kit). Read `docs/PRD.md`, `docs/TECH_STACK.md`, `docs/ARCHITECTURE.md`, and `docs/UI.md` before changing anything.

## UI rules

The UI must be elegant and pixel exact. Match `docs/design/library.png` and `docs/design/reader.png` and the tokens in `docs/UI.md`. Paper titles use the bundled serif font (Source Serif 4), everything else uses Inter. Use the 4 px grid, 1 physical pixel borders, one accent color, and the named constants in the theme module; never put literal sizes or colors in a view. Every interactive element needs hover, pressed, focused, and disabled states. Before finishing any UI change, render the affected states at 1x and 2x and inspect the screenshots against the mockups.

## Code rules

Crates under `crates/` other than `paper-app` must not depend on GPUI. PDFium calls run only on the PDF worker thread. The PDF file on disk is never modified; annotations live in SQLite. Keep raw API responses in `source_records`. Follow the polite usage rules of each metadata API (contact email in the user agent, rate limits).
