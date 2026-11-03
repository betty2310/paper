#!/usr/bin/env bash
# Builds a universal Paper.app, signs it, and zips it into dist/ for a release.
# With a Sparkle private key it also signs the zip for Sparkle and writes
# dist/appcast.xml, the update feed that SUFeedURL in Info.plist points to.
# macOS only.
#
# Usage: scripts/package-macos-app.sh <version>      (for example 0.2.0)
#
# Environment:
#   MACOS_SIGN_IDENTITY  codesign identity, such as the SHA-1 of a self-signed
#                        certificate. Default "-" (ad hoc).
#   SPARKLE_PUBLIC_KEY   EdDSA public key written to SUPublicEDKey. Without it
#                        the app cannot verify updates and Sparkle stays off.
#   SPARKLE_PRIVATE_KEY  EdDSA private key (from `generate_keys -x`). Signs the
#                        zip and writes the appcast.
#   DOWNLOAD_URL         URL of the zip in the appcast. Default: the GitHub
#                        release asset for tag v<version>.
set -euo pipefail

version="${1:?usage: $0 <version>}"
identity="${MACOS_SIGN_IDENTITY:--}"
root="$(cd "$(dirname "$0")/.." && pwd)"
package="$root/apps/macos"
dist="$root/dist"
app="$dist/Paper.app"
zip="$dist/Paper-$version.zip"
download_url="${DOWNLOAD_URL:-https://github.com/betty2310/paper/releases/download/v$version/Paper-$version.zip}"

cd "$root"
scripts/build-xcframework.sh release

build=(swift build --package-path "$package" -c release --arch arm64 --arch x86_64)
"${build[@]}"
bin="$("${build[@]}" --show-bin-path)"

rm -rf "$dist"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks"
cp "$bin/Paper" "$app/Contents/MacOS/Paper"
ditto "$bin/Sparkle.framework" "$app/Contents/Frameworks/Sparkle.framework"

plist="$app/Contents/Info.plist"
cp "$package/Paper/Info.plist" "$plist"
plutil -replace CFBundleShortVersionString -string "$version" "$plist"
plutil -replace CFBundleVersion -string "$version" "$plist"
plutil -replace SUPublicEDKey -string "${SPARKLE_PUBLIC_KEY:-}" "$plist"
if [[ -z "${SPARKLE_PUBLIC_KEY:-}" ]]; then
  plutil -remove SUFeedURL "$plist"
  echo "warning: SPARKLE_PUBLIC_KEY is not set; this build does not check for updates" >&2
fi

# Sign inside out: Sparkle's helpers, the framework, then the app. No Hardened
# Runtime on the app: without a Developer ID it would refuse to load
# Sparkle.framework (library validation), and it only matters for notarization.
sparkle="$app/Contents/Frameworks/Sparkle.framework/Versions/B"
sign() { codesign --force --sign "$identity" "$@"; }
sign "$sparkle/XPCServices/Installer.xpc"
sign --preserve-metadata=entitlements "$sparkle/XPCServices/Downloader.xpc"
sign "$sparkle/Autoupdate"
sign "$sparkle/Updater.app"
sign "$app/Contents/Frameworks/Sparkle.framework"
sign "$app"
codesign --verify --deep --strict "$app"

ditto -c -k --sequesterRsrc --keepParent "$app" "$zip"
echo "Built $zip"

if [[ -n "${SPARKLE_PRIVATE_KEY:-}" ]]; then
  sign_update="$package/.build/artifacts/sparkle/Sparkle/bin/sign_update"
  # Prints: sparkle:edSignature="…" length="…"
  signature="$(printf '%s' "$SPARKLE_PRIVATE_KEY" | "$sign_update" --ed-key-file - "$zip")"
  minimum="$(plutil -extract LSMinimumSystemVersion raw "$plist")"
  pub_date="$(LC_ALL=C date -u '+%a, %d %b %Y %H:%M:%S +0000')"
  cat > "$dist/appcast.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>Paper</title>
    <item>
      <title>Paper $version</title>
      <link>https://github.com/betty2310/paper/releases/tag/v$version</link>
      <pubDate>$pub_date</pubDate>
      <sparkle:version>$version</sparkle:version>
      <sparkle:shortVersionString>$version</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>$minimum</sparkle:minimumSystemVersion>
      <enclosure url="$download_url" type="application/octet-stream" $signature />
    </item>
  </channel>
</rss>
EOF
  echo "Wrote $dist/appcast.xml"
fi
