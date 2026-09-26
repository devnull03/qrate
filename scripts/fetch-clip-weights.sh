#!/usr/bin/env bash
# Fetch the CLIP weights visual search runs, pinned to one Hugging Face revision.
#
#   ./scripts/fetch-clip-weights.sh <destination>            download and check both files
#   ./scripts/fetch-clip-weights.sh --verify <destination>   only check files already there
#
# The release workflow runs this for every tag and attaches the result, packed by
# scripts/package-component.sh, so the weights stay downloadable if Hugging Face ever stops
# serving them. The pins are the ones in crates/visual-search and crates/components; move all
# three together.
set -euo pipefail

VERIFY_ONLY=""
if [ "${1:-}" = "--verify" ]; then
  VERIFY_ONLY=1
  shift
fi
DEST="${1:?usage: fetch-clip-weights.sh [--verify] <destination>}"

REVISION="b33cedfd0df4e43b8238760678fcc89e1a0d38b3"
BASE="https://huggingface.co/openai/clip-vit-base-patch32/resolve/$REVISION"
# name : size : SHA-256
FILES="
tokenizer.json:2224041:b556ac8c99757ffb677208af34bc8c6721572114111a6e0aaf5fa69ff0b8d842
model.safetensors:605157884:99d28a652e6ec46629ab7047a0ac82c69b1fe11e0ce672c43af65d3a9a3fc05d
"

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

mkdir -p "$DEST"
for file in $FILES; do
  name="${file%%:*}"
  rest="${file#*:}"
  size="${rest%%:*}"
  digest="${rest#*:}"
  if [ -z "$VERIFY_ONLY" ]; then
    echo "==> $BASE/$name"
    curl -fsSL --retry 3 --max-time 1800 -o "$DEST/$name" "$BASE/$name"
  fi
  actual_size="$(wc -c < "$DEST/$name" | tr -d ' ')"
  actual="$(sha256 "$DEST/$name")"
  if [ "$actual_size" != "$size" ] || [ "$actual" != "$digest" ]; then
    echo "::error::$name is $actual_size bytes with SHA-256 $actual; the pin says $size and $digest" >&2
    exit 1
  fi
done
echo "==> CLIP weights ${REVISION:0:7} checked in $DEST"
