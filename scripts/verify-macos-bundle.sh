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

if grep -q '^Signature=adhoc$' <<< "$main_details"; then
  grep -q '^Signature=adhoc$' <<< "$bridge_details" || {
    echo "Ad-hoc smoke app and runtime bridge must use the same signature policy." >&2
    exit 1
  }
else
  main_team="$(sed -n 's/^TeamIdentifier=//p' <<< "$main_details" | head -n 1)"
  bridge_team="$(sed -n 's/^TeamIdentifier=//p' <<< "$bridge_details" | head -n 1)"
  [[ -n "$main_team" && "$main_team" != "not set" && "$main_team" == "$bridge_team" ]] || {
    echo "Runtime bridge TeamIdentifier does not match the main app." >&2
    exit 1
  }
  grep -q '^Authority=Developer ID Application:' <<< "$main_details" || {
    echo "Main app Developer ID Application authority is missing." >&2
    exit 1
  }
  grep -q '^Authority=Developer ID Application:' <<< "$bridge_details" || {
    echo "Runtime bridge Developer ID Application authority is missing." >&2
    exit 1
  }
fi

if [[ "$require_notarization" == "1" ]]; then
  spctl --assess --type execute --verbose=4 "$app"
  xcrun stapler validate "$app"
fi
