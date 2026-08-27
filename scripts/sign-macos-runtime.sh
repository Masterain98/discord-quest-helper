#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS runtime signing must run on macOS." >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
policy_signing_enabled="$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).policies.macosSigningEnabled' "$script_dir/runtime-identity-tokens.json")"
if [[ "$policy_signing_enabled" != "false" ]]; then
  echo "macOS signing policy must remain disabled." >&2
  exit 1
fi
signing_enabled=false
if [[ "$signing_enabled" != "true" ]]; then
  echo "macOS signing is disabled by repository policy; payloads were left unsigned."
  exit 0
fi

if [[ $# -eq 0 ]]; then
  echo "Usage: scripts/sign-macos-runtime.sh <executable> [...]" >&2
  exit 2
fi

for executable in "$@"; do
  if [[ ! -f "$executable" || ! -x "$executable" ]]; then
    echo "Runtime executable is missing or not executable: $executable" >&2
    exit 1
  fi

  codesign --force --options runtime --sign - "$executable"
  codesign --verify --strict --verbose=4 "$executable"
done
