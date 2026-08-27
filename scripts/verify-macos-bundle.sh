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
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
IFS=$'\t' read -r main_name bridge_name signing_enabled < <(
  node -e 'const p=JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); console.log([p.identity.mainBinary, p.identity.bridgeBinary, p.policies.macosSigningEnabled].join("\t"))' "$script_dir/runtime-identity-tokens.json"
)
if [[ "$signing_enabled" != "false" ]]; then
  echo "macOS signing policy must remain disabled." >&2
  exit 1
fi
signing_enabled=false
main="$app/Contents/MacOS/$main_name"
bridge="$app/Contents/MacOS/$bridge_name"

[[ -x "$main" ]] || {
  echo "Main bundle executable is missing." >&2
  exit 1
}
[[ -x "$bridge" ]] || {
  echo "Nested runtime bridge is missing." >&2
  exit 1
}

if [[ "$signing_enabled" == "true" ]]; then
  codesign --verify --deep --strict --verbose=4 "$app"

  while IFS= read -r executable; do
    /usr/bin/file -b "$executable" | grep -q 'Mach-O' || continue
    codesign --verify --strict --verbose=4 "$executable"
  done < <(find "$app/Contents" -type f -perm +111 -print)

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
else
  echo "macOS signing verification is disabled by repository policy."
fi
