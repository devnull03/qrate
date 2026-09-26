#!/usr/bin/env bash
# Build and sign components.json for one release.
#
#   QRATE_UPDATE_SIGNING_KEY="$(cat key.pem)" ./scripts/build-components-manifest.sh dist v0.6.0
#
# Lists every component-* archive in <dist> (scripts/package-component.sh) and writes
# <dist>/components.json in the envelope update-manifest.json uses, signed by the same key. The
# payload's "kind" keeps the two apart: the app will not take one for the other.
#
#   QRATE_SIGNING_KEY_ID      the envelope's key id; qrate-dev for a development mirror, which a
#                             debug build trusts through QRATE_DEV_SIGNING_KEY (docs/dev/SETUP.md)
#   QRATE_COMPONENTS_REQUIRE  space-separated id:os:arch triples that must have an archive, so a
#                             release cannot quietly ship without one
set -euo pipefail

DIST="${1:?usage: build-components-manifest.sh <dist> <tag>}"
TAG="${2:?usage: build-components-manifest.sh <dist> <tag>}"
: "${QRATE_UPDATE_SIGNING_KEY:?QRATE_UPDATE_SIGNING_KEY (Ed25519 PEM) is required}"
KEY_ID="${QRATE_SIGNING_KEY_ID:-qrate-update-1}"

# An absolute POSIX path: GNU tar reads a Windows "C:/..." archive name as host:path.
DIST="$(cd "$DIST" && pwd)"
VERSION="${TAG#v}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
umask 077

printf '%s\n' "$QRATE_UPDATE_SIGNING_KEY" > "$WORK/key.pem"
openssl pkey -in "$WORK/key.pem" -noout -text 2>/dev/null | grep -qi ed25519 ||
  { echo "::error::the signing key is not Ed25519" >&2; exit 1; }

for triple in ${QRATE_COMPONENTS_REQUIRE:-}; do
  id="${triple%%:*}"
  rest="${triple#*:}"
  if ! compgen -G "$DIST/component-$id-*-${rest%%:*}-${rest#*:}.tar*" > /dev/null; then
    echo "::error::no $id component for ${rest%%:*} ${rest#*:} in $DIST" >&2
    exit 1
  fi
done

# Which qrate versions a component version works with, in plain version order. PDFium's ABI is
# fixed by pdfium-render and Pi speaks one bridge protocol, so those two are good for this minor
# line only: move either pin in a minor release. ffmpeg is run as a command and the weights are
# data, so they stay good from here on.
core="${VERSION%%[-+]*}"
IFS=. read -r major minor _ <<< "$core"
this_line=">=$major.$minor.0-0, <$major.$((minor + 1)).0-0"
from_here=">=$major.$minor.0-0"

components=""
for id in pdfium ffmpeg agent clip; do
  case "$id" in
    pdfium) app="$this_line"; license="Apache-2.0 OR BSD-3-Clause" ;;
    ffmpeg) app="$from_here"; license="LGPL-2.1-or-later" ;;
    agent)  app="$this_line"; license="MIT" ;;
    clip)   app="$from_here"; license="MIT" ;;
  esac
  version=""
  assets=""
  for file in "$DIST"/component-"$id"-*; do
    [ -f "$file" ] || continue
    name="$(basename "$file")"
    case "$name" in
      *.tar.gz) archive="tar.gz"; stem="${name%.tar.gz}" ;;
      *.tar)    archive="tar";    stem="${name%.tar}" ;;
      *) echo "::error::$name is not a component archive" >&2; exit 1 ;;
    esac
    # component-<id>-<version>-<os>-<arch>; the version may hold dashes, the rest cannot.
    arch="${stem##*-}"
    rest="${stem%-*}"
    os="${rest##*-}"
    rest="${rest%-*}"
    this="${rest#component-"$id"-}"
    if [ -n "$version" ] && [ "$version" != "$this" ]; then
      echo "::error::$DIST holds $id $version and $id $this; publish one version" >&2
      exit 1
    fi
    version="$this"
    size="$(wc -c < "$file" | tr -d ' ')"
    sha="$(sha256sum "$file" | cut -d' ' -f1)"
    # What unpacking writes, the bound the app enforces on it.
    installed="$(tar -xOf "$file" | wc -c | tr -d ' ')"
    [ -z "$assets" ] || assets="$assets,"
    assets="$assets
        {
          \"os\": \"$os\",
          \"arch\": \"$arch\",
          \"url\": \"https://github.com/devnull03/qrate/releases/download/$TAG/$name\",
          \"size\": $size,
          \"sha256\": \"$sha\",
          \"installed_size\": $installed,
          \"archive\": \"$archive\"
        }"
  done
  [ -n "$assets" ] || continue
  [ -z "$components" ] || components="$components,"
  components="$components
    {
      \"id\": \"$id\",
      \"version\": \"$version\",
      \"app\": \"$app\",
      \"license\": \"$license\",
      \"assets\": [$assets
      ]
    }"
done
if [ -z "$components" ]; then
  echo "::error::no component-* archives in $DIST" >&2
  exit 1
fi

cat > "$WORK/payload.json" <<JSON
{
  "kind": "qrate-components",
  "schema": 1,
  "app_version": "$VERSION",
  "published_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "components": [$components
  ]
}
JSON

openssl pkeyutl -sign -inkey "$WORK/key.pem" -rawin \
  -in "$WORK/payload.json" -out "$WORK/signature.bin"
# Checked against the public half before it is published, as the app will check it.
openssl pkey -in "$WORK/key.pem" -pubout -out "$WORK/public.pem"
openssl pkeyutl -verify -pubin -inkey "$WORK/public.pem" -rawin \
  -in "$WORK/payload.json" -sigfile "$WORK/signature.bin" > /dev/null

b64() { base64 < "$1" | tr -d '\n'; }
cat > "$DIST/components.json" <<JSON
{
  "schema": 1,
  "key_id": "$KEY_ID",
  "payload_base64": "$(b64 "$WORK/payload.json")",
  "signature_base64": "$(b64 "$WORK/signature.bin")"
}
JSON

echo "==> signed $(grep -c '"url"' "$WORK/payload.json") component assets for $TAG"
