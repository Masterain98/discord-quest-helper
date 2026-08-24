#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS smoke signing must run on macOS." >&2
  exit 2
fi

if [[ $# -ne 1 || ! -d "$1" ]]; then
  echo "Usage: scripts/sign-macos-bundle-smoke.sh <application.app>" >&2
  exit 2
fi

app="$1"
while IFS= read -r executable; do
  codesign --force --options runtime --sign - "$executable"
done < <(find "$app/Contents" -type f -perm +111 -print)

codesign --force --deep --options runtime --sign - "$app"
codesign --verify --deep --strict --verbose=4 "$app"
