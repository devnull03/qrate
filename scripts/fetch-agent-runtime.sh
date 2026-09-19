#!/usr/bin/env bash
set -euo pipefail

destination="${1:?usage: fetch-agent-runtime.sh <destination> <linux-x64|darwin-universal>}"
platform="${2:?usage: fetch-agent-runtime.sh <destination> <linux-x64|darwin-universal>}"
pi_version=0.84.2
runtime="$(cd "$(dirname "$destination")" && pwd)/$(basename "$destination")/agent"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$runtime"

fetch_pi() {
  local asset="$1" sha="$2"
  curl -fsSL "https://github.com/earendil-works/pi/releases/download/v$pi_version/pi-$asset.tar.gz" -o "$tmp/$asset.tar.gz"
  echo "$sha  $tmp/$asset.tar.gz" | shasum -a 256 -c -
  mkdir -p "$tmp/$asset"
  tar -xzf "$tmp/$asset.tar.gz" -C "$tmp/$asset"
}

case "$platform" in
  linux-x64)
    fetch_pi linux-x64 906fbe787fd225c4ac624fe7ebd5b1d55a60e0f5c7ef51795d231564f9ee1c13
    cp -R "$tmp/linux-x64/pi/." "$runtime/"
    ;;
  darwin-universal)
    fetch_pi darwin-x64 808cf02a93cd601d3ea05d47dc15c45074b120ac81decc8644cd3e40a35824e6
    fetch_pi darwin-arm64 c996e888b7f7dce44bcf24f69176ac646c44139d3916bd49a6b28e5a8c5e3a65
    cp -R "$tmp/darwin-x64/pi/." "$runtime/"
    cp -R "$tmp/darwin-arm64/pi/." "$runtime/"
    lipo -create "$tmp/darwin-x64/pi/pi" "$tmp/darwin-arm64/pi/pi" -output "$runtime/pi"
    ;;
  *) echo "unsupported agent runtime platform: $platform" >&2; exit 2 ;;
esac
chmod +x "$runtime/pi"

# The extension follows its latest release; the checksum is the digest GitHub records for the asset.
command -v jq >/dev/null || { echo "fetch-agent-runtime.sh needs jq" >&2; exit 2; }
auth=()
[ -n "${GITHUB_TOKEN:-}" ] && auth=(-H "Authorization: Bearer $GITHUB_TOKEN")
curl -fsSL ${auth[@]+"${auth[@]}"} https://api.github.com/repos/devnull03/qrate-pi-extension/releases/latest -o "$tmp/extension.json"
extension_version="$(jq -er '.tag_name | ltrimstr("v")' "$tmp/extension.json")"
asset="qrate-pi-extension-$extension_version.tar.gz"
extension_url="$(jq -er --arg name "$asset" '.assets[] | select(.name == $name) | .browser_download_url' "$tmp/extension.json")"
extension_sha="$(jq -er --arg name "$asset" '.assets[] | select(.name == $name) | .digest | ltrimstr("sha256:")' "$tmp/extension.json")"
curl -fsSL "$extension_url" -o "$tmp/extension.tar.gz"
echo "$extension_sha  $tmp/extension.tar.gz" | shasum -a 256 -c -
tar -xzf "$tmp/extension.tar.gz" -C "$tmp"
mkdir -p "$runtime/qrate-pi-extension"
cp "$tmp/qrate-pi-extension-$extension_version/SYSTEM.md" "$runtime/qrate-pi-extension/"
cp -R "$tmp/qrate-pi-extension-$extension_version/extensions" "$runtime/qrate-pi-extension/"
cp -R "$tmp/qrate-pi-extension-$extension_version/src" "$runtime/qrate-pi-extension/"
echo "Fetched Pi $pi_version and qrate-pi-extension $extension_version into $runtime"
