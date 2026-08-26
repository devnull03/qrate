#!/usr/bin/env bash
# Build and sign the update manifest for one release.
#
#   QRATE_UPDATE_SIGNING_KEY="$(cat key.pem)" ./scripts/build-update-manifest.sh dist v0.4.0
#
# Writes dist/update-manifest.json: an envelope carrying the base64 payload and its Ed25519
# signature. The payload is signed as the exact bytes that get encoded, so what the app verifies
# is what this script wrote — no re-serialization in between.
set -euo pipefail

DIST="${1:?usage: build-update-manifest.sh <dist> <tag>}"
TAG="${2:?usage: build-update-manifest.sh <dist> <tag>}"
: "${QRATE_UPDATE_SIGNING_KEY:?QRATE_UPDATE_SIGNING_KEY (Ed25519 PEM) is required}"

VERSION="${TAG#v}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
umask 077

printf '%s\n' "$QRATE_UPDATE_SIGNING_KEY" > "$WORK/key.pem"
openssl pkey -in "$WORK/key.pem" -noout -text 2>/dev/null | grep -qi ed25519 ||
  { echo "::error::the update signing key is not Ed25519" >&2; exit 1; }

# filename suffix : install kind : os : arch. The kind is what an installation's marker declares,
# so it is what decides which artifact a given install is allowed to take.
DESCRIPTORS="
-setup.exe:windows-nsis:windows:x86_64
-x86_64.zip:windows-portable:windows:x86_64
-universal.dmg:macos-bundle:macos:universal
-x86_64-linux.tar.gz:linux-tar:linux:x86_64
"

artifacts=""
found=0
for descriptor in $DESCRIPTORS; do
  suffix="${descriptor%%:*}"
  rest="${descriptor#*:}"
  kind="${rest%%:*}"
  rest="${rest#*:}"
  os="${rest%%:*}"
  arch="${rest##*:}"

  file=""
  for candidate in "$DIST"/*"$suffix"; do
    [ -f "$candidate" ] && file="$candidate"
  done
  if [ -z "$file" ]; then
    echo "::error::no release artifact matching *$suffix in $DIST" >&2
    exit 1
  fi

  name="$(basename "$file")"
  size="$(wc -c < "$file" | tr -d ' ')"
  sha="$(sha256sum "$file" | cut -d' ' -f1)"
  [ "$found" -eq 0 ] || artifacts="$artifacts,"
  artifacts="$artifacts
    {
      \"kind\": \"$kind\",
      \"os\": \"$os\",
      \"arch\": \"$arch\",
      \"url\": \"https://github.com/devnull03/qrate/releases/download/$TAG/$name\",
      \"size\": $size,
      \"sha256\": \"$sha\"
    }"
  found=$((found + 1))
done

# A prerelease tag decides the channel, not GitHub's prerelease flag — the flag is set by hand and
# an updater must not take its trust boundary from something a mis-click can change.
case "$VERSION" in
  *-*) channel="beta" ;;
  *) channel="stable" ;;
esac

cat > "$WORK/payload.json" <<JSON
{
  "channel": "$channel",
  "version": "$VERSION",
  "published_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "release_notes_url": "https://github.com/devnull03/qrate/releases/tag/$TAG",
  "artifacts": [$artifacts
  ]
}
JSON

openssl pkeyutl -sign -inkey "$WORK/key.pem" -rawin \
  -in "$WORK/payload.json" -out "$WORK/signature.bin"

b64() { base64 < "$1" | tr -d '\n'; }
cat > "$DIST/update-manifest.json" <<JSON
{
  "schema": 1,
  "key_id": "qrate-update-1",
  "payload_base64": "$(b64 "$WORK/payload.json")",
  "signature_base64": "$(b64 "$WORK/signature.bin")"
}
JSON

echo "==> signed $found artifacts for $TAG ($channel)"
