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

libraries=()
for target in "${targets[@]}"; do
  if [[ "$profile" == release ]]; then
    cargo build --package paper-ffi --target "$target" --release
  else
    cargo build --package paper-ffi --target "$target"
  fi
  libraries+=("$root/target/$target/$profile/libpaper_ffi.a")
done

universal="$root/target/universal-macos/$profile"
mkdir -p "$universal"
lipo -create "${libraries[@]}" -output "$universal/libpaper_ffi.a"

rm -rf "$package/PaperFFI.xcframework"
xcodebuild -create-xcframework \
  -library "$universal/libpaper_ffi.a" \
  -headers "$package/Headers" \
  -output "$package/PaperFFI.xcframework"

echo "Built $package/PaperFFI.xcframework"
