#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Linux" || $# -ne 5 || "$2" != "--kind" ]]; then
  echo "Usage: scripts/smoke-linux-runtime-identity.sh <x11|wayland> --kind <binary|appimage> <target> <manifest.json>" >&2
  exit 2
fi

mode="$1"
kind="$3"
target="$4"
manifest="$5"
expected="meridian"
if [[ "$mode" != "x11" && "$mode" != "wayland" ]]; then
  echo "Mode must be x11 or wayland." >&2
  exit 2
fi
if [[ "$kind" != "binary" && "$kind" != "appimage" ]]; then
  echo "Kind must be binary or appimage." >&2
  exit 2
fi
if [[ ! -x "$target" ]]; then
  echo "Smoke target must be executable." >&2
  exit 2
fi
if [[ "$kind" == "binary" && "$(basename "$target")" != "$expected" ]]; then
  echo "Binary smoke target must be named $expected." >&2
  exit 2
fi
if [[ "$kind" == "appimage" && "$target" != *.AppImage ]]; then
  echo "AppImage smoke target must use the .AppImage extension." >&2
  exit 2
fi
artifact="$kind"
[[ "$kind" == "binary" ]] && artifact="deb"

smoke_root="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/runtime-identity-smoke.XXXXXX")"
app_pid=""
runtime_pid=""
weston_pid=""
cleanup() {
  terminate_and_reap() {
    local pid="$1"
    [[ -n "$pid" ]] || return 0
    if kill -0 "$pid" 2>/dev/null; then
      kill -TERM "$pid" 2>/dev/null || true
      for _ in {1..20}; do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.1
      done
      kill -KILL "$pid" 2>/dev/null || true
    fi
    wait "$pid" 2>/dev/null || true
  }

  if [[ -n "$runtime_pid" && "$runtime_pid" != "$app_pid" ]]; then
    terminate_and_reap "$runtime_pid"
  fi
  if [[ -n "$app_pid" ]]; then
    terminate_and_reap "$app_pid"
  fi
  if [[ -n "$weston_pid" ]]; then
    terminate_and_reap "$weston_pid"
  fi
  if [[ -d "$smoke_root" && "$(basename "$smoke_root")" == runtime-identity-smoke.* ]]; then
    rm -rf -- "$smoke_root"
  fi
}
trap cleanup EXIT

is_descendant_of() {
  local candidate="$1"
  local ancestor="$2"
  local current="$candidate"
  local parent=""
  for _ in {1..32}; do
    [[ "$current" == "$ancestor" ]] && return 0
    [[ -r "/proc/$current/status" ]] || return 1
    parent="$(awk '/^PPid:/ { print $2 }' "/proc/$current/status")"
    [[ -n "$parent" && "$parent" != "0" && "$parent" != "$current" ]] || return 1
    current="$parent"
  done
  return 1
}

find_runtime_pid() {
  local candidate=""
  local comm=""
  local exe=""
  for proc in /proc/[0-9]*; do
    candidate="${proc##*/}"
    is_descendant_of "$candidate" "$app_pid" || continue
    [[ -r "$proc/comm" ]] || continue
    comm="$(tr -d '\n' 2>/dev/null < "$proc/comm" || true)"
    exe="$(readlink -f "$proc/exe" 2>/dev/null || true)"
    if [[ "$comm" == "$expected" || "$(basename "$exe")" == "$expected" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

appimage_residual_fields() {
  local pid="$1"
  local environment=""
  local separator=""
  environment="$(tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null || true)"
  printf '['
  for name in APPIMAGE APPDIR ARGV0; do
    if grep -q "^${name}=" <<< "$environment"; then
      printf '%s"%s"' "$separator" "$name"
      separator=", "
    fi
  done
  printf ']'
}

if [[ "$mode" == "wayland" ]]; then
  export XDG_RUNTIME_DIR="$smoke_root/xdg-runtime"
  mkdir -p "$XDG_RUNTIME_DIR"
  chmod 700 "$XDG_RUNTIME_DIR"
  export WAYLAND_DISPLAY="wayland-identity"
  weston --backend=headless-backend.so --socket="$WAYLAND_DISPLAY" --idle-time=0 \
    >"$smoke_root/weston.log" 2>&1 &
  weston_pid=$!
  for _ in {1..100}; do
    [[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]] && break
    kill -0 "$weston_pid" 2>/dev/null || {
      cat "$smoke_root/weston.log" >&2
      exit 1
    }
    sleep 0.1
  done
  [[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]] || {
    echo "Weston did not create its Wayland socket." >&2
    exit 1
  }
  unset DISPLAY
  export XDG_SESSION_TYPE=wayland
  GDK_BACKEND=wayland WAYLAND_DEBUG=client WEBKIT_DISABLE_DMABUF_RENDERER=1 \
    NO_AT_BRIDGE=1 RUNTIME_IDENTITY_AUDIT=1 "$target" \
    >"$smoke_root/app.log" 2>"$smoke_root/protocol.log" &
else
  [[ -n "${DISPLAY:-}" ]] || {
    echo "X11 mode requires DISPLAY (run under xvfb-run)." >&2
    exit 2
  }
  GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 NO_AT_BRIDGE=1 \
    RUNTIME_IDENTITY_AUDIT=1 "$target" \
    >"$smoke_root/app.log" 2>"$smoke_root/protocol.log" &
fi
app_pid=$!

for _ in {1..150}; do
  runtime_pid="$(find_runtime_pid || true)"
  [[ -n "$runtime_pid" ]] && break
  kill -0 "$app_pid" 2>/dev/null || {
    cat "$smoke_root/app.log" >&2
    cat "$smoke_root/protocol.log" >&2
    exit 1
  }
  sleep 0.1
done

[[ -n "$runtime_pid" ]] || {
  echo "Could not locate the $expected runtime in the launched $kind process tree." >&2
  cat "$smoke_root/app.log" >&2
  cat "$smoke_root/protocol.log" >&2
  exit 1
}

comm="$(tr -d '\n' < "/proc/$runtime_pid/comm")"
exe="$(readlink -f "/proc/$runtime_pid/exe")"
if [[ "$comm" != "$expected" || "$(basename "$exe")" != "$expected" ]]; then
  echo "Unexpected runtime process identity: comm=$comm exe=$exe" >&2
  exit 1
fi
appimage_residuals="$(appimage_residual_fields "$runtime_pid")"
if [[ "$kind" == "appimage" ]]; then
  for name in APPIMAGE APPDIR ARGV0; do
    [[ "$appimage_residuals" == *"\"$name\""* ]] || {
      echo "AppImage runtime did not preserve the standard $name field." >&2
      exit 1
    }
  done
fi

if [[ "$mode" == "x11" ]]; then
  window=""
  for _ in {1..150}; do
    window="$(xdotool search --onlyvisible --pid "$runtime_pid" 2>/dev/null | head -n 1 || true)"
    [[ -n "$window" ]] && break
    kill -0 "$runtime_pid" 2>/dev/null || exit 1
    sleep 0.1
  done
  [[ -n "$window" ]] || {
    echo "No visible X11 window was created." >&2
    exit 1
  }
  wm_class="$(xprop -id "$window" WM_CLASS)"
  [[ "$wm_class" == *"\"$expected\""* ]] || {
    echo "WM_CLASS does not contain $expected: $wm_class" >&2
    exit 1
  }
  window_count="$(xdotool search --onlyvisible --pid "$runtime_pid" 2>/dev/null | sort -u | wc -l | tr -d ' ')"
  [[ "$window_count" == "1" ]] || {
    echo "Expected one visible application window, found $window_count." >&2
    exit 1
  }
  printf '{\n  "platform": "linux",\n  "artifact": "%s",\n  "session": "x11",\n  "launchPid": %s,\n  "runtimePid": %s,\n  "process": "%s",\n  "comm": "%s",\n  "executableBasename": "%s",\n  "wmClass": "%s",\n  "appImageResidualFields": %s,\n  "visibleWindows": %s\n}\n' \
    "$artifact" "$app_pid" "$runtime_pid" "$expected" "$comm" "$(basename "$exe")" "$expected" \
    "$appimage_residuals" "$window_count" > "$manifest"
else
  for _ in {1..150}; do
    grep -q 'set_app_id' "$smoke_root/protocol.log" 2>/dev/null && break
    kill -0 "$runtime_pid" 2>/dev/null || exit 1
    sleep 0.1
  done
  grep -q 'set_app_id.*"meridian"' "$smoke_root/protocol.log" || {
    echo "Wayland protocol log did not set app_id to meridian." >&2
    tail -100 "$smoke_root/protocol.log" >&2
    exit 1
  }
  app_id_lines="$(grep 'set_app_id' "$smoke_root/protocol.log" || true)"
  if grep -vq '"meridian"' <<< "$app_id_lines"; then
    echo "Wayland protocol log contains an unexpected app_id." >&2
    printf '%s\n' "$app_id_lines" >&2
    exit 1
  fi
  printf '{\n  "platform": "linux",\n  "artifact": "%s",\n  "session": "wayland",\n  "launchPid": %s,\n  "runtimePid": %s,\n  "process": "%s",\n  "comm": "%s",\n  "executableBasename": "%s",\n  "appId": "%s",\n  "appImageResidualFields": %s\n}\n' \
    "$artifact" "$app_pid" "$runtime_pid" "$expected" "$comm" "$(basename "$exe")" "$expected" \
    "$appimage_residuals" > "$manifest"
fi
