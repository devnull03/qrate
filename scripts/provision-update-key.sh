#!/usr/bin/env bash
# Establish the update signing trust root, once.
#
#   ./scripts/provision-update-key.sh [path-to-qrate-site]
#
# Generates the Ed25519 pair, patches the two public copies (the updater crate and the site's feed
# route), and hands the private half to GitHub. The private key is written only to a temporary
# file that this script deletes, and is never printed — so it exists in exactly one durable place,
# the release-signing environment secret.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SITE="${1:-$REPO_ROOT/../qrate-site}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
umask 077

openssl genpkey -algorithm ed25519 -out "$WORK/key.pem"
openssl pkey -in "$WORK/key.pem" -pubout -outform DER > "$WORK/pub.der"
# An Ed25519 SubjectPublicKeyInfo is a 12-byte header followed by the 32 raw key bytes.
tail -c 32 "$WORK/pub.der" > "$WORK/pub.raw"
[ "$(wc -c < "$WORK/pub.raw" | tr -d ' ')" = 32 ] ||
  { echo "unexpected Ed25519 public key length" >&2; exit 1; }

# rustfmt packs 16 elements to a line at this width; matching it keeps `cargo fmt --check` green.
rust_rows="$(xxd -p -c 1 "$WORK/pub.raw" | awk '
  { row = row sprintf("0x%s, ", $0)
    if (NR % 16 == 0) { print "    " substr(row, 1, length(row) - 1); row = "" } }
  END { if (row != "") print "    " substr(row, 1, length(row) - 1) }
')"

LIB="$REPO_ROOT/crates/updater/src/lib.rs"
awk -v rows="$rust_rows" '
  /^const UPDATE_PUBLIC_KEY/ { print; print rows; skip = 1; next }
  skip && /^\];/ { skip = 0 }
  skip { next }
  { print }
' "$LIB" > "$WORK/lib.rs"
mv "$WORK/lib.rs" "$LIB"
echo "==> patched crates/updater/src/lib.rs"

# The site stores the same key as a JWK, so it needs the base64url form of the same 32 bytes.
jwk_x="$(base64 < "$WORK/pub.raw" | tr -d '\n=' | tr '+/' '-_')"
FEED="$SITE/src/pages/updates/[channel].json.ts"
if [ -f "$FEED" ]; then
  sed -i "s|x: '[A-Za-z0-9_-]*'|x: '$jwk_x'|" "$FEED"
  echo "==> patched $FEED"
else
  echo "==> no qrate-site checkout at $SITE — set its feed route to x = $jwk_x" >&2
fi

# `gh secret set --env` fails against an environment that does not exist yet.
gh api -X PUT repos/devnull03/qrate/environments/release-signing > /dev/null
before="$(gh api repos/devnull03/qrate/environments/release-signing/secrets \
  --jq '.secrets[] | select(.name == "QRATE_UPDATE_SIGNING_KEY") | .updated_at' 2>/dev/null || true)"
gh secret set QRATE_UPDATE_SIGNING_KEY --env release-signing < "$WORK/key.pem"

# The private key is deleted on exit, so an upload that quietly did nothing leaves a public key
# nobody can ever sign for. Prove the secret actually moved before trusting it.
after="$(gh api repos/devnull03/qrate/environments/release-signing/secrets \
  --jq '.secrets[] | select(.name == "QRATE_UPDATE_SIGNING_KEY") | .updated_at')"
if [ -z "$after" ] || [ "$after" = "$before" ]; then
  echo "::error::the signing secret did not change — the patched public keys have no private half" >&2
  echo "         revert the two patched files and run this again" >&2
  exit 1
fi
echo "==> stored QRATE_UPDATE_SIGNING_KEY in the release-signing environment ($after)"
echo "==> commit the two patched files, then run cargo test -p updater"
