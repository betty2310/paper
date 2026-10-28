#!/usr/bin/env bash
# Regenerates the Swift bindings for paper-ffi. Commit the result.
#
# UniFFI reads the API description that the proc macros embedded in the built
# library, so this builds paper-ffi first. Works on macOS and Linux.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
package="$root/apps/macos/PaperEngine"

cd "$root"
cargo build --package paper-ffi

case "$(uname -s)" in
  Darwin) library="$root/target/debug/libpaper_ffi.dylib" ;;
  *) library="$root/target/debug/libpaper_ffi.so" ;;
esac

bindgen() { cargo run --quiet --package uniffi-bindgen -- "$@"; }

# Swift source: the API the app sees (`import PaperEngine`).
bindgen --swift-sources "$library" "$package/Sources/PaperEngine"
# C header and module map: the low level symbols, packed into the XCFramework.
bindgen --headers "$library" "$package/Headers"
bindgen --modulemap --module-name paper_ffiFFI --modulemap-filename module.modulemap "$library" "$package/Headers"

echo "Bindings written to $package"
