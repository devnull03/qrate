#!/usr/bin/env bash
#
# Regenerate platform icons from assets/icons/app-icon.png (the single source of truth).
#
# - macOS: produces assets/icons/app-icon.icns via sips + iconutil (no extra installs).
# - .ico:  produced via ImageMagick if available (`magick`/`convert`). On Windows,
#          use scripts/gen-icons.ps1 instead (no ImageMagick required).
#
# To change the app icon: replace app-icon.png, run this script, and commit the result.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src="$root/assets/icons/app-icon.png"
[ -f "$src" ] || { echo "Source icon not found: $src" >&2; exit 1; }

# ---- macOS .icns -----------------------------------------------------------
if command -v iconutil >/dev/null 2>&1 && command -v sips >/dev/null 2>&1; then
  iconset="$(mktemp -d)/AppIcon.iconset"
  mkdir -p "$iconset"
  for s in 16 32 128 256 512; do
    sips -z "$s" "$s"       "$src" --out "$iconset/icon_${s}x${s}.png"     >/dev/null
    sips -z $((s*2)) $((s*2)) "$src" --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
  done
  iconutil -c icns "$iconset" -o "$root/assets/icons/app-icon.icns"
  echo "Wrote assets/icons/app-icon.icns"
else
  echo "skip .icns: sips/iconutil not found (macOS only)"
fi

# ---- Windows .ico ----------------------------------------------------------
magick=""
command -v magick  >/dev/null 2>&1 && magick="magick"
[ -z "$magick" ] && command -v convert >/dev/null 2>&1 && magick="convert"
if [ -n "$magick" ]; then
  "$magick" "$src" -define icon:auto-resize=256,128,64,48,32,16 "$root/assets/icons/app-icon.ico"
  echo "Wrote assets/icons/app-icon.ico"
else
  echo "skip .ico: ImageMagick not found (use scripts/gen-icons.ps1 on Windows)"
fi
