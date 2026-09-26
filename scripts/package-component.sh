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
# Reproducible: the same inputs give the same bytes in every release, so an app update finds the
# installed copy's SHA-256 in the new manifest and keeps it instead of downloading it again.
# Python rather than tar, because GNU and BSD tar share no flags for sorting and timestamps.
PYTHON="$(command -v python3 || command -v python)" || fail "python is needed to pack $ID"
"$PYTHON" - "$SRC" "$OUT" <<'PY'
import gzip, os, sys, tarfile

src, out = sys.argv[1], sys.argv[2]

def normal(info):
    info.mtime = 0
    info.uid = info.gid = 0
    info.uname = info.gname = ""
    info.mode = 0o755 if info.isdir() or info.mode & 0o111 else 0o644
    return info

entries = []
for root, dirs, files in os.walk(src, followlinks=True):
    dirs.sort()
    for name in dirs + sorted(files):
        path = os.path.join(root, name)
        entries.append((os.path.relpath(path, src).replace(os.sep, "/"), path))
entries.sort()

with open(out, "wb") as raw:
    stream = gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) if out.endswith(".gz") else raw
    with tarfile.open(fileobj=stream, mode="w", format=tarfile.PAX_FORMAT) as tar:
        for name, path in entries:
            info = normal(tar.gettarinfo(os.path.realpath(path), arcname=name))
            if info.isfile():
                with open(path, "rb") as data:
                    tar.addfile(info, data)
            else:
                tar.addfile(info)
    if stream is not raw:
        stream.close()
PY

if tar -tvf "$OUT" | grep -qv '^[-d]'; then
  tar -tvf "$OUT" | grep -v '^[-d]' >&2
  fail "$OUT holds links or special files, which qrate will not unpack"
fi
echo "==> $(basename "$OUT"): $(wc -c < "$OUT" | tr -d ' ') bytes"
