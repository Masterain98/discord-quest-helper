#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS bundle verification must run on macOS." >&2
  exit 2
fi

if [[ $# -ne 1 || ! -d "$1" ]]; then
  echo "Usage: scripts/verify-macos-bundle.sh <application.app>" >&2
  exit 2
fi

app="$1"
require_notarization="${REQUIRE_NOTARIZATION:-0}"

codesign --verify --deep --strict --verbose=4 "$app"

while IFS= read -r executable; do
  codesign --verify --strict --verbose=4 "$executable"
done < <(find "$app/Contents" -type f -perm +111 -print)

if [[ "$require_notarization" == "1" ]]; then
  spctl --assess --type execute --verbose=4 "$app"
  xcrun stapler validate "$app"
fi
