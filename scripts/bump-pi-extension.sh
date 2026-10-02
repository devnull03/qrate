#!/usr/bin/env bash
# Repin qrate-pi-extension in both fetch scripts.
#
#   ./scripts/bump-pi-extension.sh           # the extension's latest release
#   ./scripts/bump-pi-extension.sh v0.3.1    # a named one
#
# The checksum is taken from the tarball this downloads, not from the release's metadata, so the
# pin records the bytes you can inspect before committing. Needs an authenticated `gh`.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo=devnull03/qrate-pi-extension
tag="${1:-$(gh release view --repo "$repo" --json tagName --jq .tagName)}"
export VERSION="${tag#v}"
asset="qrate-pi-extension-$VERSION.tar.gz"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
gh release download "$tag" --repo "$repo" --pattern "$asset" --dir "$tmp"
SHA="$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)"
export SHA

perl -pi -e 's/^extension_version=[^\r\n]*/extension_version=$ENV{VERSION}/;
             s/^extension_sha=[^\r\n]*/extension_sha=$ENV{SHA}/' "$here/fetch-agent-runtime.sh"
perl -pi -e 's/^\$extensionVersion = [^\r\n]*/\$extensionVersion = "$ENV{VERSION}"/;
             s/^\$extensionSha256 = [^\r\n]*/\$extensionSha256 = "$ENV{SHA}"/' "$here/fetch-agent-runtime.ps1"

# A pattern that stops matching would otherwise leave the old pin in place and say nothing.
grep -q "^extension_version=$VERSION" "$here/fetch-agent-runtime.sh" &&
  grep -q "^extension_sha=$SHA" "$here/fetch-agent-runtime.sh" &&
  grep -q "^\$extensionVersion = \"$VERSION\"" "$here/fetch-agent-runtime.ps1" &&
  grep -q "^\$extensionSha256 = \"$SHA\"" "$here/fetch-agent-runtime.ps1" ||
  { echo "bump-pi-extension.sh could not find the pin lines to rewrite" >&2; exit 1; }

echo "Pinned qrate-pi-extension $VERSION ($SHA)"
