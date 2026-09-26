#!/usr/bin/env bash
# Pack one optional component for one platform (docs/dev/components-plan.md).
#
#   ./scripts/package-component.sh <id> <src dir> <os> <arch> <dist>
#
#   id    pdfium | ffmpeg | agent | clip
#   os    windows | linux | macos | any
#   arch  x86_64 | aarch64 | any
#
# Writes <dist>/component-<id>-<version>-<os>-<arch>.tar.gz (a plain .tar for clip, whose weights
# do not compress) holding the *contents* of <src dir>: that folder is what an install unpacks
# into <data dir>/components/<id>/<version>. The version is read from the pin in the script that
# fetched the input, so an archive cannot carry a version it is not.
#
# qrate refuses links and special files when it unpacks, so symbolic links are packed as the
# files they point to, and an archive holding anything else fails here rather than on a user's
# machine.
set -euo pipefail

usage="usage: package-component.sh <id> <src dir> <os> <arch> <dist>"
ID="${1:?$usage}"
SRC="${2:?$usage}"
OS="${3:?$usage}"
ARCH="${4:?$usage}"
DIST="${5:?$usage}"
HERE="$(cd "$(dirname "$0")" && pwd)"

fail() {
  echo "::error::$*" >&2
  exit 1
}

# The value of NAME="value" or NAME=value in one of the fetch scripts.
pin() {
  local value
  value="$(sed -n "s/^$1=\"\{0,1\}\([^\"]*\)\"\{0,1\}\$/\1/p" "$HERE/$2" | head -1)"
  [ -n "$value" ] || fail "no $1 pin in scripts/$2"
  printf '%s' "$value"
}

EXT=tar.gz
case "$ID" in
  pdfium) VERSION="$(pin PDFIUM_RELEASE fetch-binaries.sh | tr / -)" ;;
  ffmpeg)
    VERSION="$(pin FFMPEG_BUILD fetch-binaries.sh)"
    # LGPL: the notice travels with the binary.
    [ -f "$SRC/LICENSE.txt" ] || fail "the ffmpeg component needs its LICENSE.txt in $SRC"
    ;;
  agent)
    VERSION="$(pin pi_version fetch-agent-runtime.sh)-ext.$(pin extension_version fetch-agent-runtime.sh)"
    ;;
  clip)
    VERSION="$(pin REVISION fetch-clip-weights.sh | cut -c1-7)"
    EXT=tar
    ;;
  *) fail "unknown component $ID" ;;
esac
case "$OS" in windows|linux|macos|any) ;; *) fail "unknown os $OS" ;; esac
case "$ARCH" in x86_64|aarch64|any) ;; *) fail "unknown arch $ARCH" ;; esac
[ -d "$SRC" ] && [ -n "$(ls -A "$SRC")" ] || fail "$SRC is empty or missing"

mkdir -p "$DIST"
OUT="$(cd "$DIST" && pwd)/component-$ID-$VERSION-$OS-$ARCH.$EXT"
flags=-chf
if [ "$EXT" = tar.gz ]; then flags=-czhf; fi
# COPYFILE_DISABLE keeps macOS tar from adding ._ resource-fork files.
COPYFILE_DISABLE=1 tar "$flags" "$OUT" -C "$SRC" .

if tar -tvf "$OUT" | grep -qv '^[-d]'; then
  tar -tvf "$OUT" | grep -v '^[-d]' >&2
  fail "$OUT holds links or special files, which qrate will not unpack"
fi
echo "==> $(basename "$OUT"): $(wc -c < "$OUT" | tr -d ' ') bytes"
