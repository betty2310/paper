# Paper

A native, local first desktop app for managing scientific papers. The logic is a Rust engine, and each platform gets a native app on top of it. Version 1.0 is the macOS app (AppKit, SwiftUI, PDFKit); Windows and Linux follow on the same engine. Enter a DOI, get the metadata and the open access PDF, organize papers into collections, read and annotate PDFs in a built in viewer, write notes, and copy citations.

Status: milestone 1 of the [roadmap](docs/ROADMAP.md), engine side. The Rust engine stores the library in SQLite (items, creators, collections, tags, notes, full text search), looks metadata up from Crossref with doi.org content negotiation as the fallback, supports delete and undo, and exposes it all through UniFFI. The macOS app is still the spike 1 window on top of the new engine; the sidebar, detail panel, and menus of milestone 1 are next.

## Run the demo on macOS

You need Rust (`rustup`) and Xcode 16 or later.

```sh
scripts/build-xcframework.sh          # Rust → apps/macos/PaperEngine/PaperFFI.xcframework
cd apps/macos && swift run Paper      # builds and opens the app
```

Search filters the table on every keystroke through a synchronous engine call. Pasting a DOI such as `10.1038/nature14539` and pressing Add DOI runs an `async` engine call on an engine thread, and the new row appears through the `EngineObserver` callback.

After changing the engine API in `crates/paper-engine` and `crates/paper-ffi`, run `scripts/generate-bindings.sh`, commit the regenerated files in `apps/macos/PaperEngine`, and rebuild the XCFramework. `cargo test --workspace` runs the engine tests on any OS.

## Releases

Pushing a tag such as `v0.2.0` runs [`.github/workflows/release.yml`](.github/workflows/release.yml) on a macOS runner. It calls `scripts/package-macos-app.sh 0.2.0`, which builds a universal `Paper.app`, signs it, zips it, and writes a Sparkle `appcast.xml`. Then it publishes both files as the GitHub release. A tag run cannot restore caches that other tags saved, so [`.github/workflows/rust-cache.yml`](.github/workflows/rust-cache.yml) compiles the Rust dependencies on `main` (when a `Cargo.toml` or `Cargo.lock` changes, and weekly) and the release restores them. A tag pushed before that run finishes builds them from scratch. The app checks `releases/latest/download/appcast.xml` once a day and offers the update. Paper › Check for Updates… checks right away. Sparkle installs an update only when the zip's EdDSA signature matches the public key built into the running app.

There is no Apple Developer ID yet. The app is signed with a self-signed certificate (or ad hoc if none is set up) and is not notarized.

One-time setup, on your Mac:

```sh
# Sparkle update keys. Paths are under apps/macos/.build after one `swift build`.
bin=apps/macos/.build/artifacts/sparkle/Sparkle/bin
$bin/generate_keys                         # stores the key in your login keychain
$bin/generate_keys -p | gh variable set SPARKLE_PUBLIC_KEY
$bin/generate_keys -x /tmp/sparkle-key && gh secret set SPARKLE_PRIVATE_KEY < /tmp/sparkle-key && rm /tmp/sparkle-key

# Self-signed code signing certificate, so every release has the same identity.
scripts/create-signing-cert.sh '<password>'
base64 -i signing/paper-signing.p12 | gh secret set MACOS_CERT_P12
gh secret set MACOS_CERT_PASSWORD --body '<password>'
security add-trusted-cert -r trustRoot -p codeSign -k ~/Library/Keychains/login.keychain-db signing/paper-signing.cer
```

Keep the Sparkle key and `signing/` safe. If you lose the Sparkle key, installed copies cannot verify new updates.

The first install needs one manual step because the app is not notarized. Unzip the release into `/Applications`, then run `xattr -dr com.apple.quarantine /Applications/Paper.app` (or right-click › Open). Updates through Sparkle need no extra step.

## Documents

- [Product requirements](docs/PRD.md): what the MVP does and does not do, and the performance targets.
- [Technology decisions](docs/TECH_STACK.md): the chosen crates and the alternatives that were rejected.
- [Architecture](docs/ARCHITECTURE.md): repository layout, the engine API, database schema, threading, the PDFKit reader, and the macOS app structure.
- [UI design](docs/UI.md): layout, typography, colors, states, shortcuts, with mockups in `docs/design/`.
- [Roadmap](docs/ROADMAP.md): spikes first, then five milestones.
- Research notes: [GPUI](docs/research/gpui.md), [Rust engine with native UI shells](docs/research/native-ui-rust-core.md), [PDF engines](docs/research/pdf-engines.md), [DOI and PDF sources](docs/research/doi-and-pdf-sources.md).
