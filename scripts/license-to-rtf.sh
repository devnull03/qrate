#!/usr/bin/env bash
# Render LICENSE.md as the RTF the MSI's license dialog needs.
#
#   ./scripts/license-to-rtf.sh LICENSE.md dist/license.rtf
#
# WiX only accepts RTF here, and its built-in placeholder is lorem ipsum — so a missing conversion
# ships a real installer asking people to accept filler text. Generated rather than committed so
# the dialog always shows the licence in the repository.
set -euo pipefail

SOURCE="${1:?usage: license-to-rtf.sh <license> <output.rtf>}"
OUTPUT="${2:?usage: license-to-rtf.sh <license> <output.rtf>}"

mkdir -p "$(dirname "$OUTPUT")"
{
  printf '{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fnil Segoe UI;}}\\fs18\n'
  # Backslashes and braces are RTF syntax, so they escape first or they corrupt the document.
  sed -e 's/\\/\\\\/g' -e 's/{/\\{/g' -e 's/}/\\}/g' -e 's/$/\\par/' "$SOURCE"
  printf '}\n'
} > "$OUTPUT"

echo "==> wrote $OUTPUT ($(wc -c < "$OUTPUT" | tr -d ' ') bytes)"
