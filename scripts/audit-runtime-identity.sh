#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

if ! command -v node >/dev/null 2>&1; then
  echo "Runtime identity audit requires Node.js." >&2
  exit 2
fi

exec node "$SCRIPT_DIR/audit-runtime-identity.mjs" "$@"
