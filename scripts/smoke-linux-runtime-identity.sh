#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Linux" || $# -ne 3 ]]; then
  echo "Usage: scripts/smoke-linux-runtime-identity.sh <x11|wayland> <meridian> <manifest.json>" >&2
  exit 2
fi

mode="$1"
binary="$2"
manifest="$3"
expected="meridian"
if [[ "$mode" != "x11" && "$mode" != "wayland" ]]; then
  echo "Mode must be x11 or wayland." >&2
  exit 2
fi
if [[ ! -x "$binary" || "$(basename "$binary")" != "$expected" ]]; then
  echo "Smoke target must be an executable named $expected." >&2
  exit 2
fi

smoke_root="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/runtime-identity-smoke.XXXXXX")"
app_pid=""
weston_pid=""
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
  if [[ -n "$weston_pid" ]] && kill -0 "$weston_pid" 2>/dev/null; then
    kill -TERM "$weston_pid" 2>/dev/null || true
    wait "$weston_pid" 2>/dev/null || true
  fi
  if [[ -d "$smoke_root" && "$(basename "$smoke_root")" == runtime-identity-smoke.* ]]; then
    rm -rf -- "$smoke_root"
  fi
}
trap cleanup EXIT

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
  GDK_BACKEND=wayland WAYLAND_DEBUG=client WEBKIT_DISABLE_DMABUF_RENDERER=1 \
    NO_AT_BRIDGE=1 RUNTIME_IDENTITY_AUDIT=1 "$binary" \
    >"$smoke_root/app.log" 2>"$smoke_root/protocol.log" &
else
  [[ -n "${DISPLAY:-}" ]] || {
    echo "X11 mode requires DISPLAY (run under xvfb-run)." >&2
    exit 2
  }
  GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 NO_AT_BRIDGE=1 \
    RUNTIME_IDENTITY_AUDIT=1 "$binary" \
    >"$smoke_root/app.log" 2>"$smoke_root/protocol.log" &
fi
app_pid=$!

for _ in {1..150}; do
  [[ -e "/proc/$app_pid/exe" ]] && break
  kill -0 "$app_pid" 2>/dev/null || {
    cat "$smoke_root/app.log" >&2
    cat "$smoke_root/protocol.log" >&2
    exit 1
  }
  sleep 0.1
done

comm="$(tr -d '\n' < "/proc/$app_pid/comm")"
exe="$(readlink -f "/proc/$app_pid/exe")"
if [[ "$comm" != "$expected" || "$(basename "$exe")" != "$expected" ]]; then
  echo "Unexpected runtime process identity: comm=$comm exe=$exe" >&2
  exit 1
fi

if [[ "$mode" == "x11" ]]; then
  window=""
  for _ in {1..150}; do
    window="$(xdotool search --onlyvisible --pid "$app_pid" 2>/dev/null | head -n 1 || true)"
    [[ -n "$window" ]] && break
    kill -0 "$app_pid" 2>/dev/null || exit 1
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
  window_count="$(xdotool search --onlyvisible --pid "$app_pid" 2>/dev/null | sort -u | wc -l | tr -d ' ')"
  [[ "$window_count" == "1" ]] || {
    echo "Expected one visible application window, found $window_count." >&2
    exit 1
  }
  printf '{\n  "platform": "linux",\n  "session": "x11",\n  "process": "%s",\n  "comm": "%s",\n  "wmClass": "%s",\n  "visibleWindows": %s\n}\n' \
    "$expected" "$comm" "$expected" "$window_count" > "$manifest"
else
  for _ in {1..150}; do
    grep -q 'set_app_id' "$smoke_root/protocol.log" 2>/dev/null && break
    kill -0 "$app_pid" 2>/dev/null || exit 1
    sleep 0.1
  done
  grep -q 'set_app_id.*"meridian"' "$smoke_root/protocol.log" || {
    echo "Wayland protocol log did not set app_id to meridian." >&2
    tail -100 "$smoke_root/protocol.log" >&2
    exit 1
  }
  if grep 'set_app_id' "$smoke_root/protocol.log" | grep -vq '"meridian"'; then
    echo "Wayland protocol log contains an unexpected app_id." >&2
    grep 'set_app_id' "$smoke_root/protocol.log" >&2
    exit 1
  fi
  printf '{\n  "platform": "linux",\n  "session": "wayland",\n  "process": "%s",\n  "comm": "%s",\n  "appId": "%s"\n}\n' \
    "$expected" "$comm" "$expected" > "$manifest"
fi
