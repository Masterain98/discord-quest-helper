#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Darwin" || $# -ne 2 || ! -d "$1" ]]; then
  echo "Usage: scripts/smoke-macos-runtime-identity.sh <application.app> <manifest.json>" >&2
  exit 2
fi

app="$1"
manifest="$2"
executable_name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")"
[[ "$executable_name" == "meridian" ]] || {
  echo "CFBundleExecutable must be meridian." >&2
  exit 1
}
binary="$app/Contents/MacOS/$executable_name"
[[ -x "$binary" ]] || {
  echo "Bundle executable is missing." >&2
  exit 1
}

smoke_root="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/runtime-identity-smoke.XXXXXX")"
app_pid=""
cleanup() {
  if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
    kill -TERM "$app_pid" 2>/dev/null || true
    for _ in {1..20}; do
      kill -0 "$app_pid" 2>/dev/null || break
      sleep 0.1
    done
    kill -KILL "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
  if [[ -d "$smoke_root" && "$(basename "$smoke_root")" == runtime-identity-smoke.* ]]; then
    rm -rf -- "$smoke_root"
  fi
}
trap cleanup EXIT

RUNTIME_IDENTITY_AUDIT=1 "$binary" >"$smoke_root/app.log" 2>&1 &
app_pid=$!
for _ in {1..150}; do
  command_path="$(ps -p "$app_pid" -o comm= 2>/dev/null | xargs || true)"
  [[ -n "$command_path" ]] && break
  kill -0 "$app_pid" 2>/dev/null || {
    cat "$smoke_root/app.log" >&2
    exit 1
  }
  sleep 0.1
done

[[ "$command_path" == "$binary" ]] || {
  echo "macOS process is not running from the signed bundle: $command_path" >&2
  exit 1
}
[[ "$command_path" != /tmp/* && "$command_path" != "${TMPDIR:-/tmp}"/* ]] || {
  echo "macOS process unexpectedly runs as a bare temporary executable." >&2
  exit 1
}
process_count="$(ps -axo comm= | awk -v target="$binary" '$0 == target { count += 1 } END { print count + 0 }')"
[[ "$process_count" == "1" ]] || {
  echo "Expected one main application process, found $process_count." >&2
  exit 1
}

launch_services=""
for _ in {1..50}; do
  launch_services="$(lsappinfo find bundleID=com.masterain.discord-quest-helper 2>/dev/null || true)"
  [[ -n "$launch_services" ]] && break
  sleep 0.1
done
[[ -n "$launch_services" ]] || {
  echo "The application did not register its public bundle with Launch Services." >&2
  exit 1
}
launch_info="$(lsappinfo info -only name -only pid "$launch_services")"
[[ "$launch_info" == *'"LSDisplayName"="Discord Quest Helper"'* ]] || {
  echo "Launch Services public display name changed." >&2
  exit 1
}
[[ "$launch_info" == *"\"pid\"=$app_pid"* ]] || {
  echo "Launch Services registered a different main process." >&2
  exit 1
}

codesign --verify --deep --strict --verbose=4 "$app"
printf '{\n  "platform": "macos",\n  "bundleExecutable": "%s",\n  "processPath": "%s",\n  "temporaryCopy": false,\n  "mainProcessCount": %s,\n  "launchServicesDisplayName": "Discord Quest Helper",\n  "strictSignature": true\n}\n' \
  "$executable_name" '${APP}/Contents/MacOS/meridian' "$process_count" > "$manifest"
