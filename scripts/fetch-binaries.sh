#!/usr/bin/env bash
# Fetch the native binaries the preview tiers load at runtime.
#
# qrate does not link either of these. PDFium is loaded dynamically and ffmpeg is run as a
# subprocess, so a checkout without them still builds, launches and passes its tests — PDFs and
# videos simply show a type icon. That is why this is a script you run rather than a build step
# that can fail your build.
#
#   ./scripts/fetch-binaries.sh [destination]
#
# Destination defaults to target/debug, which is where `cargo run` looks: both tiers search
# beside the running executable first, then fall back to the system copy. For a release build or
# an installer, point this at the directory the packaged executable will live in.
#
# ffmpeg is left to the system package manager on macOS and Linux, where every package manager
# has it and shipping our own would mean carrying ~50 MB per platform for no gain. On Windows,
# where there is no such default, it is fetched.
#
# Two environment variables, both for CI:
#
#   QRATE_PDFIUM_ARCH=univ   Fetch the macOS universal build, for a lipo'd bundle.
#   QRATE_SKIP_FFMPEG=1      PDFium only. CI wants the small, quick download so the PDF tests
#                            exercise real rendering; the runners already provide ffmpeg.
set -euo pipefail

DEST="${1:-target/debug}"
mkdir -p "$DEST"
DEST="$(cd "$DEST" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) PLATFORM=win ;;
  Darwin)               PLATFORM=mac ;;
  *)                    PLATFORM=linux ;;
esac

case "$(uname -m)" in
  x86_64|amd64)  ARCH=x64 ;;
  arm64|aarch64) ARCH=arm64 ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
ARCH="${QRATE_PDFIUM_ARCH:-$ARCH}"

echo "==> platform: $PLATFORM-$ARCH, destination: $DEST"

# --- PDFium (Apache-2.0 or BSD-3) --------------------------------------------------------------
PDFIUM_URL="https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-$PLATFORM-$ARCH.tgz"
echo "==> pdfium: $PDFIUM_URL"
curl -sSL --fail --max-time 300 -o "$WORK/pdfium.tgz" "$PDFIUM_URL"
tar xzf "$WORK/pdfium.tgz" -C "$WORK"
# The archive puts the shared library under bin/ or lib/ depending on the platform, so search the
# whole extraction rather than guessing. Verified rather than assumed: a silent miss here would
# produce a release that shows an icon for every PDF, with nothing in the build log to say why.
found=0
while IFS= read -r lib; do
  cp "$lib" "$DEST/"
  found=1
done < <(find "$WORK" -type f \
  \( -name 'pdfium.dll' -o -name 'libpdfium.so' -o -name 'libpdfium.dylib' \) 2>/dev/null)

if [ "$found" -eq 0 ]; then
  echo "::error::no PDFium library inside $PDFIUM_URL — archive layout changed?" >&2
  exit 1
fi
echo "    installed $(tr '\n' ' ' < "$WORK/VERSION" 2>/dev/null || echo 'pdfium')"

# --- ffmpeg (LGPL/GPL depending on build; shipped unmodified as a separate executable) ----------
if [ -n "${QRATE_SKIP_FFMPEG:-}" ]; then
  echo "==> ffmpeg: skipped (QRATE_SKIP_FFMPEG set)"
elif [ "$PLATFORM" = "win" ]; then
  # gyan.dev's redirect always points at the current release; the GitHub assets carry the version
  # in their filename, so there is no stable "latest" URL there to use instead.
  FFMPEG_URL="https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
  echo "==> ffmpeg: $FFMPEG_URL"
  if curl -sSL --fail --max-time 600 -o "$WORK/ffmpeg.zip" "$FFMPEG_URL"; then
    unzip -q -o "$WORK/ffmpeg.zip" -d "$WORK/ffmpeg"
    find "$WORK/ffmpeg" -name 'ffmpeg.exe' -exec cp {} "$DEST/" \;
    echo "    installed ffmpeg.exe"
  else
    echo "    ffmpeg download failed; video previews will fall back to an icon" >&2
  fi
elif command -v ffmpeg >/dev/null 2>&1; then
  echo "==> ffmpeg: already on PATH ($(command -v ffmpeg))"
else
  echo "==> ffmpeg: not found. Install it with your package manager:" >&2
  echo "      macOS: brew install ffmpeg     Debian/Ubuntu: apt install ffmpeg" >&2
fi

echo "==> done. Contents of $DEST:"
ls -1 "$DEST" | grep -Ei 'pdfium|ffmpeg' || echo "    (nothing installed)"
