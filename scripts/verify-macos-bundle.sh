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

codesign --verify --deep --strict --verbose=4 "$app"

while IFS= read -r executable; do
  /usr/bin/file -b "$executable" | grep -q 'Mach-O' || continue
  codesign --verify --strict --verbose=4 "$executable"
done < <(find "$app/Contents" -type f -perm +111 -print)

bridge="$app/Contents/MacOS/waybridge"
[[ -x "$bridge" ]] || {
  echo "Nested runtime bridge is missing." >&2
  exit 1
}
main_details="$(codesign -dvv "$app" 2>&1)"
bridge_details="$(codesign -dvv "$bridge" 2>&1)"
grep -q 'flags=.*runtime' <<< "$main_details" || {
  echo "Main app signature is missing hardened runtime." >&2
  exit 1
}
grep -q 'flags=.*runtime' <<< "$bridge_details" || {
  echo "Runtime bridge signature is missing hardened runtime." >&2
  exit 1
}

grep -q '^Signature=adhoc$' <<< "$main_details" || {
  echo "Main app must use the configured ad-hoc signature policy." >&2
  exit 1
}
grep -q '^Signature=adhoc$' <<< "$bridge_details" || {
  echo "Runtime bridge must use the configured ad-hoc signature policy." >&2
  exit 1
}
