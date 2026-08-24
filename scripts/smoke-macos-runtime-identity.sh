#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Darwin" || $# -ne 2 || ! -d "$1" ]]; then
  echo "Usage: scripts/smoke-macos-runtime-identity.sh <application.app> <manifest.json>" >&2
  exit 2
fi

app="$(cd "$1" && pwd -P)"
manifest="$2"
info="$app/Contents/Info.plist"
executable_name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$info")"
bundle_identifier="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$info")"
display_name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleDisplayName' "$info")"
[[ "$executable_name" == "meridian" ]] || {
  echo "CFBundleExecutable must be meridian." >&2
  exit 1
}
[[ "$bundle_identifier" == "com.masterain.discord-quest-helper" ]] || {
  echo "CFBundleIdentifier changed." >&2
  exit 1
}
[[ "$display_name" == "Discord Quest Helper" ]] || {
  echo "CFBundleDisplayName changed." >&2
  exit 1
}
binary="$app/Contents/MacOS/$executable_name"
[[ -x "$binary" ]] || {
  echo "Bundle executable is missing." >&2
  exit 1
}

smoke_root="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/runtime-identity-smoke.XXXXXX")"
app_pid=""
open_wait_pid=""

stop_app() {
  if [[ -z "$app_pid" ]]; then
    app_pid="$(find_exact_process_pid || true)"
  fi
  if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
    kill -TERM "$app_pid" 2>/dev/null || true
    for _ in {1..20}; do
      kill -0 "$app_pid" 2>/dev/null || break
      sleep 0.1
    done
    kill -KILL "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
  app_pid=""
  if [[ -n "$open_wait_pid" ]]; then
    wait "$open_wait_pid" 2>/dev/null || true
    open_wait_pid=""
  fi
}

cleanup() {
  stop_app
  if [[ -d "$smoke_root" && "$(basename "$smoke_root")" == runtime-identity-smoke.* ]]; then
    rm -rf -- "$smoke_root"
  fi
}
trap cleanup EXIT

process_path_for_pid() {
  ps -p "$1" -o comm= 2>/dev/null | xargs || true
}

find_exact_process_pid() {
  local candidate=""
  while read -r candidate; do
    [[ -n "$candidate" ]] || continue
    if [[ "$(process_path_for_pid "$candidate")" == "$binary" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done < <(pgrep -x "$executable_name" 2>/dev/null || true)
  return 1
}

wait_for_exact_process() {
  local pid=""
  for _ in {1..150}; do
    pid="$(find_exact_process_pid)"
    if [[ -n "$pid" ]]; then
      printf '%s\n' "$pid"
      return 0
    fi
    sleep 0.1
  done
  return 1
}

verify_process_path() {
  local pid="$1"
  local command_path=""
  command_path="$(process_path_for_pid "$pid")"
  [[ "$command_path" == "$binary" ]] || {
    echo "macOS process is not running from the signed bundle: $command_path" >&2
    exit 1
  }
  [[ "$command_path" != /tmp/* && "$command_path" != "${TMPDIR:-/tmp}"/* ]] || {
    echo "macOS process unexpectedly runs as a bare temporary executable." >&2
    exit 1
  }
}

verify_launch_services() {
  local pid="$1"
  local launch_services=""
  local launch_info=""
  for _ in {1..50}; do
    launch_services="$(lsappinfo find "bundleID=$bundle_identifier" 2>/dev/null || true)"
    [[ -n "$launch_services" ]] && break
    sleep 0.1
  done
  [[ -n "$launch_services" ]] || {
    echo "The application did not register with Launch Services." >&2
    exit 1
  }
  launch_info="$(lsappinfo info -only name -only bundleID -only pid "$launch_services")"
  [[ "$launch_info" == *'"LSDisplayName"="Discord Quest Helper"'* ]] || {
    echo "Launch Services public display name changed." >&2
    exit 1
  }
  [[ "$launch_info" == *"\"pid\"=$pid"* ]] || {
    echo "Launch Services registered a different main process." >&2
    exit 1
  }
}

# Direct Mach-O launch verifies the bundle executable without LaunchServices
# choosing or rewriting the executable path.
RUNTIME_IDENTITY_AUDIT=1 "$binary" >"$smoke_root/direct.log" 2>&1 &
app_pid=$!
for _ in {1..150}; do
  [[ "$(process_path_for_pid "$app_pid")" == "$binary" ]] && break
  kill -0 "$app_pid" 2>/dev/null || {
    cat "$smoke_root/direct.log" >&2
    exit 1
  }
  sleep 0.1
done
verify_process_path "$app_pid"
direct_pid="$app_pid"
verify_launch_services "$direct_pid"
stop_app

# `open` follows the same LaunchServices path used by Finder. `-W` only keeps
# the launcher available for deterministic cleanup; `-n` forces a new app.
RUNTIME_IDENTITY_AUDIT=1 open -n -W "$app" >"$smoke_root/open.log" 2>&1 &
open_wait_pid=$!
app_pid="$(wait_for_exact_process || true)"
[[ -n "$app_pid" ]] || {
  cat "$smoke_root/open.log" >&2
  echo "LaunchServices did not start the expected bundle executable." >&2
  exit 1
}
verify_process_path "$app_pid"
verify_launch_services "$app_pid"
launch_services_pid="$app_pid"

process_count=0
while read -r candidate; do
  [[ -n "$candidate" ]] || continue
  if [[ "$(process_path_for_pid "$candidate")" == "$binary" ]]; then
    process_count=$((process_count + 1))
  fi
done < <(pgrep -x "$executable_name" 2>/dev/null || true)
[[ "$process_count" == "1" ]] || {
  echo "Expected one main application process, found $process_count." >&2
  exit 1
}

codesign --verify --deep --strict --verbose=4 "$app"
printf '{\n  "platform": "macos",\n  "bundleDirectoryName": "%s",\n  "bundleIdentifier": "%s",\n  "bundleExecutable": "%s",\n  "directLaunch": {\n    "pid": %s,\n    "processPath": "${APP}/Contents/MacOS/meridian"\n  },\n  "launchServicesLaunch": {\n    "method": "open -n",\n    "pid": %s,\n    "displayName": "Discord Quest Helper",\n    "processPath": "${APP}/Contents/MacOS/meridian"\n  },\n  "temporaryCopy": false,\n  "mainProcessCount": %s,\n  "strictSignature": true\n}\n' \
  "$(basename "$app")" "$bundle_identifier" "$executable_name" "$direct_pid" \
  "$launch_services_pid" "$process_count" > "$manifest"
