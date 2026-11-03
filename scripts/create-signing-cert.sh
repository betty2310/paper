#!/usr/bin/env bash
# Creates a self-signed code signing certificate for release builds, valid for
# ten years, as signing/paper-signing.p12 (certificate and key) and
# signing/paper-signing.cer (certificate only, to trust on your Mac). Every
# release signed with it keeps the same identity, so macOS keeps the app's
# permissions and keychain access across updates. Works on macOS and Linux.
#
# Usage: scripts/create-signing-cert.sh <p12-password>
set -euo pipefail

password="${1:?usage: $0 <p12-password>}"
out="signing"
mkdir -p "$out"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cat > "$work/cert.conf" <<'EOF'
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = Paper Self-Signed
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
subjectKeyIdentifier = hash
EOF

openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$work/cert.conf" \
  -keyout "$work/key.pem" -out "$work/cert.pem"
# SHA-1 and 3DES, the PKCS#12 algorithms that macOS `security import` reads.
openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" -name "Paper Self-Signed" \
  -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1 \
  -passout "pass:$password" -out "$out/paper-signing.p12"
openssl x509 -in "$work/cert.pem" -outform DER -out "$out/paper-signing.cer"

echo "Wrote $out/paper-signing.p12 and $out/paper-signing.cer. Keep them out of Git."
