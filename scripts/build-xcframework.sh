#!/usr/bin/env bash
# Builds paper-ffi as a universal static library and wraps it with the
# generated header and module map into PaperFFI.xcframework, which the
# PaperEngine Swift package links. macOS only.
#
# Usage: scripts/build-xcframework.sh [debug|release]   (default: release)
set -euo pipefail

profile="${1:-release}"
root="$(cd "$(dirname "$0")/.." && pwd)"
package="$root/apps/macos/PaperEngine"
targets=(aarch64-apple-darwin x86_64-apple-darwin)
export MACOSX_DEPLOYMENT_TARGET=13.0

cd "$root"
rustup target add "${targets[@]}"

# One cargo run for both targets, so one architecture's long single-threaded
# steps (codegen of the largest crates, LTO of paper-ffi) overlap the other's.
# A cold release build peaks near 7 GB when both targets compile pdf_oxide.
args=(--package paper-ffi)
libraries=()
for target in "${targets[@]}"; do
  args+=(--target "$target")
  libraries+=("$root/target/$target/$profile/libpaper_ffi.a")
done
if [[ "$profile" == release ]]; then
  args+=(--release)
fi
cargo build "${args[@]}"

universal="$root/target/universal-macos/$profile"
mkdir -p "$universal"
lipo -create "${libraries[@]}" -output "$universal/libpaper_ffi.a"

rm -rf "$package/PaperFFI.xcframework"
xcodebuild -create-xcframework \
  -library "$universal/libpaper_ffi.a" \
  -headers "$package/Headers" \
  -output "$package/PaperFFI.xcframework"

echo "Built $package/PaperFFI.xcframework"
