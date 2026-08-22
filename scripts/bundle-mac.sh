#!/usr/bin/env bash
#
# Assemble a macOS .app bundle from a (universal) binary and package it as a .dmg.
#
# Usage:
#   scripts/bundle-mac.sh <path-to-universal-binary> <version>
#
# Produces:
#   dist/qrate.app
#   dist/qrate-<version>-universal.dmg
#
# The app icon is generated from assets/icons/app-icon.png (sips + iconutil, both
# preinstalled on macOS) — replace that PNG to change the icon. The bundle is
# UNSIGNED; users must right-click > Open (or clear the quarantine bit) the first time.
set -euo pipefail

BIN="${1:?usage: bundle-mac.sh <binary> <version>}"
VERSION="${2:?usage: bundle-mac.sh <binary> <version>}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app_name="qrate"
executable="qrate"
dist="$root/dist"
app="$dist/${app_name}.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$dist"

# ---- Executable ------------------------------------------------------------
cp "$BIN" "$app/Contents/MacOS/$executable"
chmod +x "$app/Contents/MacOS/$executable"

# ---- Preview sidecars ------------------------------------------------------
# PDFium is dlopen'd and ffmpeg is spawned, both looked for beside the executable first (see
# scripts/fetch-binaries.sh). Optional by design: without them PDFs and video fall back to a type
# icon, so a bundle built without them is degraded, never broken.
for sidecar in libpdfium.dylib ffmpeg; do
  if [ -f "$(dirname "$BIN")/$sidecar" ]; then
    cp "$(dirname "$BIN")/$sidecar" "$app/Contents/MacOS/$sidecar"
    chmod +x "$app/Contents/MacOS/$sidecar"
    echo "bundled $sidecar"
  fi
done

# ---- Embedded agent --------------------------------------------------------
# Kept in Resources rather than MacOS because only Pi itself is executable; the extension, prompt,
# and skill are versioned data that qrate passes to it explicitly.
if [ -d "$(dirname "$BIN")/agent" ]; then
  cp -R "$(dirname "$BIN")/agent" "$app/Contents/Resources/agent"
  chmod +x "$app/Contents/Resources/agent/pi"
  echo "bundled Pi agent runtime"
fi

# ---- Info.plist (substitute version) ---------------------------------------
sed "s/__VERSION__/${VERSION}/g" "$root/assets/macos/Info.plist" \
  > "$app/Contents/Info.plist"

# ---- Icon: app-icon.png -> AppIcon.icns ------------------------------------
iconset="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$iconset"
src_png="$root/assets/icons/app-icon.png"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s"          "$src_png" --out "$iconset/icon_${s}x${s}.png"     >/dev/null
  sips -z $((s*2)) $((s*2))  "$src_png" --out "$iconset/icon_${s}x${s}@2x.png"  >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"

# ---- .dmg ------------------------------------------------------------------
dmg="$dist/qrate-${VERSION}-universal.dmg"
staging="$(mktemp -d)/dmg"
mkdir -p "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
rm -f "$dmg"
hdiutil create \
  -volname "$app_name" \
  -srcfolder "$staging" \
  -ov -format UDZO \
  "$dmg" >/dev/null

echo "Built: $app"
echo "Built: $dmg"
