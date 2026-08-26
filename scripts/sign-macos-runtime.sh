#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS runtime signing must run on macOS." >&2
  exit 2
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
