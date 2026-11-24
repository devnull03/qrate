#!/usr/bin/env bash
set -euo pipefail

destination="${1:?usage: fetch-agent-runtime.sh <destination> <linux-x64|darwin-universal|darwin-x64|darwin-arm64>}"
platform="${2:?usage: fetch-agent-runtime.sh <destination> <linux-x64|darwin-universal|darwin-x64|darwin-arm64>}"
pi_version=1.6.0
extension_version=0.7.0
extension_sha=8e4493e7aaa41770321800d2abb706b6febe9ea213c0e242a88d321fa448ddb2
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
    fetch_pi linux-x64 ef77dbe4379b3bdeb425a1e1b862fa3c915c255000dfe4965c98eee96a5765d1
    cp -R "$tmp/linux-x64/pi/." "$runtime/"
    ;;
  # One architecture each, for the per-Mac agent component.
  darwin-x64)
    fetch_pi darwin-x64 5b69cd87c3b00e74cce2d1499df3b7a1ec73632f11abf64696d110f0f4dc4192
    cp -R "$tmp/darwin-x64/pi/." "$runtime/"
    ;;
  darwin-arm64)
    fetch_pi darwin-arm64 6429287b7ae98d726ec577815d14fd680e7c587e89ecc5153b4a145dc4adba33
    cp -R "$tmp/darwin-arm64/pi/." "$runtime/"
    ;;
  darwin-universal)
    fetch_pi darwin-x64 5b69cd87c3b00e74cce2d1499df3b7a1ec73632f11abf64696d110f0f4dc4192
    fetch_pi darwin-arm64 6429287b7ae98d726ec577815d14fd680e7c587e89ecc5153b4a145dc4adba33
    cp -R "$tmp/darwin-x64/pi/." "$runtime/"
    cp -R "$tmp/darwin-arm64/pi/." "$runtime/"
    lipo -create "$tmp/darwin-x64/pi/pi" "$tmp/darwin-arm64/pi/pi" -output "$runtime/pi"
    ;;
  *) echo "unsupported agent runtime platform: $platform" >&2; exit 2 ;;
esac
chmod +x "$runtime/pi"

curl -fsSL "https://github.com/devnull03/qrate-pi-extension/releases/download/v$extension_version/qrate-pi-extension-$extension_version.tar.gz" -o "$tmp/extension.tar.gz"
echo "$extension_sha  $tmp/extension.tar.gz" | shasum -a 256 -c -
tar -xzf "$tmp/extension.tar.gz" -C "$tmp"
mkdir -p "$runtime/qrate-pi-extension"
cp "$tmp/qrate-pi-extension-$extension_version/SYSTEM.md" "$runtime/qrate-pi-extension/"
cp -R "$tmp/qrate-pi-extension-$extension_version/extensions" "$runtime/qrate-pi-extension/"
cp -R "$tmp/qrate-pi-extension-$extension_version/src" "$runtime/qrate-pi-extension/"
echo "Fetched Pi $pi_version and qrate-pi-extension $extension_version into $runtime"
