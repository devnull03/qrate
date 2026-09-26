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
# Both downloads are pinned to one release and checked against its SHA-256, because they are also
# what the optional components are packaged from (docs/dev/components-plan.md). To move a pin,
# change the release and every hash below together; GitHub lists each asset's digest on its
# release page and in `gh api repos/<owner>/<repo>/releases/tags/<tag>`.
#
# PDFium's build has to match the API `pdfium-render` binds: its `pdfium_latest` feature names it
# (crates/preview/Cargo.toml). ffmpeg is BtbN's LGPL build, since qrate only decodes. It comes from
# a month-end autobuild because BtbN keeps those for about two years and the daily ones for two
# weeks. macOS has no such build, so there it stays `brew install ffmpeg`.
#
# Environment variables, all for CI:
#
#   QRATE_PDFIUM_ARCH=univ   Fetch the macOS universal build, for a lipo'd bundle.
#   QRATE_SKIP_FFMPEG=1      PDFium only. CI wants the small, quick download so the PDF tests
#                            exercise real rendering; the runners already provide ffmpeg.
#   QRATE_STRICT_BINARIES=1  Fail if an expected release sidecar cannot be downloaded or found.
#   QRATE_FFMPEG_LICENSE=<file>  Also copy the ffmpeg build's LICENSE.txt to <file>, for the
#                            ffmpeg component (scripts/package-component.sh).
set -euo pipefail

PDFIUM_RELEASE="chromium/7881"
FFMPEG_RELEASE="autobuild-2026-08-31-13-27"
FFMPEG_BUILD="n8.1.2-50-g1a748fe2cd"
FFMPEG_BRANCH="8.1"

pdfium_sha256() {
  case "$1" in
    win-x64)     echo 73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08 ;;
    win-arm64)   echo d3035d4d2cacac6ecd1a2ece197a3d702a1b2a58466276b9f870b8cb278a9d84 ;;
    linux-x64)   echo 1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d ;;
    linux-arm64) echo ee7f7b7d5468958336a818c1cd580bdd20972846b7377b13f9a923d92d1d4674 ;;
    mac-x64)     echo 6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b ;;
    mac-arm64)   echo 52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40 ;;
    mac-univ)    echo df451a413c3609585e84a4a91110a9bc889cff05fe3b2db0ed817c9e90c3f7d3 ;;
  esac
}

ffmpeg_sha256() {
  case "$1" in
    win64)      echo f6274bbd9c247f9e90c1bbed066b03ed4a3907cece2fb91be6dd352393936365 ;;
    winarm64)   echo 0ad4d6e7342d6d77bbae4ac230964ebd51bdd6192414392e5f521d295b39b111 ;;
    linux64)    echo 7d6d93e9c39e0e461feb13c118e91e4eec2515e4da3a01d4ad6790996731bbee ;;
    linuxarm64) echo 56b37b6f2832ba37bd4979ae5c4521ae718efa41846a0d3ecfbbe492137c66f6 ;;
  esac
}

# Downloads $1 to $2 and checks it against $3. A mismatch is treated like a failed download: the
# file is never installed.
fetch_pinned() {
  curl -sSL --fail --max-time 600 -o "$2" "$1" || return 1
  local actual
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$2" | cut -d' ' -f1)"
  else
    actual="$(shasum -a 256 "$2" | cut -d' ' -f1)"
  fi
  if [ "$actual" != "$3" ]; then
    echo "::error::$1 has SHA-256 $actual, but the pin says $3" >&2
    return 1
  fi
}

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
  x86_64|amd64)  MACHINE=x64 ;;
  arm64|aarch64) MACHINE=arm64 ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
ARCH="${QRATE_PDFIUM_ARCH:-$MACHINE}"

echo "==> platform: $PLATFORM-$ARCH, destination: $DEST"

# --- PDFium (Apache-2.0 or BSD-3) --------------------------------------------------------------
PDFIUM_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/$PDFIUM_RELEASE/pdfium-$PLATFORM-$ARCH.tgz"
PDFIUM_SHA256="$(pdfium_sha256 "$PLATFORM-$ARCH")"
echo "==> pdfium: $PDFIUM_URL"
if [ -z "$PDFIUM_SHA256" ]; then
  echo "    no pinned PDFium build for $PLATFORM-$ARCH" >&2
  if [ -n "${QRATE_STRICT_BINARIES:-}" ]; then exit 1; fi
elif fetch_pinned "$PDFIUM_URL" "$WORK/pdfium.tgz" "$PDFIUM_SHA256"; then
  tar xzf "$WORK/pdfium.tgz" -C "$WORK"
else
  echo "    PDFium download failed; PDF previews will fall back to an icon" >&2
  if [ -n "${QRATE_STRICT_BINARIES:-}" ]; then exit 1; fi
fi
# The archive puts the shared library under bin/ or lib/ depending on the platform, so search the
# whole extraction rather than guessing. Verified rather than assumed: a silent miss here would
# produce a release that shows an icon for every PDF, with nothing in the build log to say why.
found=0
while IFS= read -r lib; do
  cp "$lib" "$DEST/"
  found=1
done < <(find "$WORK" -type f \
  \( -name 'pdfium.dll' -o -name 'libpdfium.so' -o -name 'libpdfium.dylib' \) 2>/dev/null)

if [ "$found" -eq 0 ] && [ -n "${QRATE_STRICT_BINARIES:-}" ]; then
  echo "::error::no PDFium library inside $PDFIUM_URL — archive layout changed?" >&2
  exit 1
fi
echo "    installed $(tr '\n' ' ' < "$WORK/VERSION" 2>/dev/null || echo 'pdfium')"

# --- ffmpeg (LGPL; shipped unmodified as a separate executable) ---------------------------------
case "$PLATFORM-$MACHINE" in
  win-x64)     FFMPEG_TARGET=win64;      FFMPEG_EXT=zip ;;
  win-arm64)   FFMPEG_TARGET=winarm64;   FFMPEG_EXT=zip ;;
  linux-x64)   FFMPEG_TARGET=linux64;    FFMPEG_EXT=tar.xz ;;
  linux-arm64) FFMPEG_TARGET=linuxarm64; FFMPEG_EXT=tar.xz ;;
  *)           FFMPEG_TARGET="" ;;
esac
FFMPEG_EXE=ffmpeg
if [ "$PLATFORM" = "win" ]; then FFMPEG_EXE=ffmpeg.exe; fi

if [ -n "${QRATE_SKIP_FFMPEG:-}" ]; then
  echo "==> ffmpeg: skipped (QRATE_SKIP_FFMPEG set)"
elif [ -n "$FFMPEG_TARGET" ]; then
  FFMPEG_ASSET="ffmpeg-$FFMPEG_BUILD-$FFMPEG_TARGET-lgpl-$FFMPEG_BRANCH.$FFMPEG_EXT"
  FFMPEG_URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/$FFMPEG_RELEASE/$FFMPEG_ASSET"
  echo "==> ffmpeg: $FFMPEG_URL"
  if fetch_pinned "$FFMPEG_URL" "$WORK/$FFMPEG_ASSET" "$(ffmpeg_sha256 "$FFMPEG_TARGET")"; then
    mkdir -p "$WORK/ffmpeg"
    if [ "$FFMPEG_EXT" = "zip" ]; then
      unzip -q -o "$WORK/$FFMPEG_ASSET" -d "$WORK/ffmpeg"
    else
      tar xJf "$WORK/$FFMPEG_ASSET" -C "$WORK/ffmpeg"
    fi
    find "$WORK/ffmpeg" -type f -path "*/bin/$FFMPEG_EXE" -exec cp {} "$DEST/" \;
    # The ffmpeg component carries the build's LGPL notice. The bundle beside the exe does not change.
    if [ -n "${QRATE_FFMPEG_LICENSE:-}" ]; then
      license="$(find "$WORK/ffmpeg" -maxdepth 2 -type f -name LICENSE.txt | head -1)"
      if [ -z "$license" ]; then
        echo "::error::no LICENSE.txt inside $FFMPEG_URL" >&2
        exit 1
      fi
      mkdir -p "$(dirname "$QRATE_FFMPEG_LICENSE")"
      cp "$license" "$QRATE_FFMPEG_LICENSE"
    fi
    echo "    installed $FFMPEG_EXE $FFMPEG_BUILD"
  else
    echo "    ffmpeg download failed; video previews will fall back to an icon" >&2
    if [ -n "${QRATE_STRICT_BINARIES:-}" ]; then exit 1; fi
  fi
  if [ -n "${QRATE_STRICT_BINARIES:-}" ] && [ ! -f "$DEST/$FFMPEG_EXE" ]; then
    echo "::error::$FFMPEG_EXE is missing from the strict release payload" >&2
    exit 1
  fi
elif command -v ffmpeg >/dev/null 2>&1; then
  echo "==> ffmpeg: already on PATH ($(command -v ffmpeg))"
else
  echo "==> ffmpeg: not found. Install it with Homebrew: brew install ffmpeg" >&2
fi

echo "==> done. Contents of $DEST:"
ls -1 "$DEST" | grep -Ei 'pdfium|ffmpeg' || echo "    (nothing installed)"
