#!/usr/bin/env bash
# Checks the live plugin registry against the root key this repo compiles in
# (ROOT_PUBKEY in backend/src/services/plugins/signing.rs). Fails when the
# registry is signed by a different key, so a root rotation that skipped this
# repo fails CI instead of every instance's registry sync.
#
#   scripts/verify-registry-root.sh [registry-url]
#
# Needs bash, curl and OpenSSL 3 or later (Ed25519 with -rawin).
set -euo pipefail

base="${1:-https://nosdesk.com/registry}"
base="${base%/}"
repo="$(cd "$(dirname "$0")/.." && pwd)"
src="$repo/backend/src/services/plugins/signing.rs"

key=$(sed -n 's/^pub const ROOT_PUBKEY: &str = "\([A-Za-z0-9+/=]*\)";$/\1/p' "$src")
if [ -z "$key" ]; then
  echo "::error::no ROOT_PUBKEY constant found in $src"
  exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

printf '%s' "$key" | base64 -d > "$work/root.raw"
if [ "$(wc -c < "$work/root.raw" | tr -d ' ')" != 32 ]; then
  echo "::error::ROOT_PUBKEY is not a 32-byte Ed25519 key"
  exit 1
fi
fp=$(openssl dgst -sha256 -r "$work/root.raw" | cut -c1-16)
# SubjectPublicKeyInfo for Ed25519 is a fixed 12-byte header plus the raw key.
{ printf '\x30\x2a\x30\x05\x06\x03\x2b\x65\x70\x03\x21\x00'; cat "$work/root.raw"; } > "$work/root.der"
openssl pkey -pubin -inform DER -in "$work/root.der" -out "$work/root.pem"

fetch() {
  curl -fsS --retry 3 --retry-all-errors --max-time 30 -o "$work/$1" "$base/$1"
}

echo "Trusted root: $key (fingerprint $fp)"
failed=0

fetch root.pub
served=$(tr -d '[:space:]' < "$work/root.pub")
if [ "$served" = "$key" ]; then
  echo "ok root.pub"
else
  echo "::error::$base/root.pub is $served, but this repo trusts $key (fingerprint $fp)"
  failed=1
fi

for doc in publishers.json index.json; do
  fetch "$doc"
  fetch "$doc.sig"
  { printf 'nosdesk-registry-v1:'; cat "$work/$doc"; } > "$work/$doc.signed"
  tr -d '[:space:]' < "$work/$doc.sig" | base64 -d > "$work/$doc.sigraw"
  if openssl pkeyutl -verify -pubin -inkey "$work/root.pem" -rawin \
      -in "$work/$doc.signed" -sigfile "$work/$doc.sigraw" > /dev/null 2>&1; then
    echo "ok $doc"
  else
    echo "::error::$base/$doc is signed by a different root key than this repo trusts (fingerprint $fp)"
    failed=1
  fi
done

exit "$failed"
