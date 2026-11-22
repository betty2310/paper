# Paper

A native reference manager for scientific papers: a Rust engine under `crates/` and a macOS app under `apps/macos/`.

## UI

The UI must be pixel perfect, elegant, and polished.

## Invariants

Never modify the PDF file on disk. After an engine API change, regenerate the Swift bindings with `scripts/generate-bindings.sh` and commit them.
