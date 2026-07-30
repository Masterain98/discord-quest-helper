#!/usr/bin/env bash
# Discord Quest Helper - Ubuntu Build Script
# This script builds and packages the application for Ubuntu/Linux.

set -Eeuo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_ROOT"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
WHITE='\033[1;37m'
GRAY='\033[0;90m'
NC='\033[0m'

INSTALL_DEPS=false
SKIP_RUNNER_BUILD=false
SKIP_TAURI_BUILD=false

usage() {
    cat <<'EOF'
Usage: ./build-ubuntu.sh [options]

Options:
  --install-deps       Install missing Ubuntu build dependencies with apt.
  --skip-runner-build  Skip building the runner and CDP launcher sidecars.
  --skip-tauri-build   Skip building and bundling the Tauri application.
  -h, --help           Show this help message.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --install-deps)
            INSTALL_DEPS=true
            shift
            ;;
        --skip-runner-build)
            SKIP_RUNNER_BUILD=true
            shift
            ;;
        --skip-tauri-build)
            SKIP_TAURI_BUILD=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf '%bUnknown option: %s%b\n\n' "$RED" "$1" "$NC" >&2
            usage >&2
            exit 1
            ;;
    esac
done

printf '%b========================================%b\n' "$CYAN" "$NC"
printf '%b  Discord Quest Helper Build Script%b\n' "$CYAN" "$NC"
printf '%b  (Ubuntu x86_64)%b\n' "$CYAN" "$NC"
printf '%b========================================%b\n\n' "$CYAN" "$NC"

if [[ "$(uname -s)" != "Linux" ]]; then
    printf '%bError: This script must be run on Linux.%b\n' "$RED" "$NC" >&2
    exit 1
fi

if ! command -v apt-get >/dev/null 2>&1 || ! command -v dpkg-query >/dev/null 2>&1; then
    printf '%bError: This helper currently supports Ubuntu and other apt-based Debian distributions.%b\n' "$RED" "$NC" >&2
    exit 1
fi

if [[ "$(uname -m)" != "x86_64" ]]; then
    printf '%bWarning: This script is tested on x86_64; detected %s.%b\n' "$YELLOW" "$(uname -m)" "$NC"
fi

REQUIRED_PACKAGES=(
    build-essential
    pkg-config
    libwebkit2gtk-4.1-dev
    libayatana-appindicator3-dev
    librsvg2-dev
    libssl-dev
    libxdo-dev
    curl
    wget
    file
    patchelf
    xdg-utils
)

missing_packages=()
for package in "${REQUIRED_PACKAGES[@]}"; do
    if ! dpkg-query -W -f='${Status}' "$package" 2>/dev/null | grep -q '^install ok installed$'; then
        missing_packages+=("$package")
    fi
done

if [[ ${#missing_packages[@]} -gt 0 ]]; then
    if [[ "$INSTALL_DEPS" == true ]]; then
        printf '%bInstalling missing Ubuntu build dependencies...%b\n' "$YELLOW" "$NC"
        sudo apt-get update
        sudo apt-get install -y "${missing_packages[@]}"
    else
        printf '%bMissing Ubuntu build dependencies:%b\n' "$RED" "$NC" >&2
        printf '  %s\n' "${missing_packages[@]}" >&2
        printf '\nRun this script again with --install-deps, or install them manually:\n' >&2
        printf '  sudo apt-get update\n  sudo apt-get install -y' >&2
        printf ' %q' "${missing_packages[@]}" >&2
        printf '\n' >&2
        exit 1
    fi
fi

for tool in cargo rustc pnpm; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf '%bError: Required command not found: %s%b\n' "$RED" "$tool" "$NC" >&2
        exit 1
    fi
done

if [[ ! -d "$PROJECT_ROOT/node_modules" ]]; then
    printf '%bError: Frontend dependencies are not installed. Run `pnpm install` first.%b\n' "$RED" "$NC" >&2
    exit 1
fi

VERSION_FILE="$PROJECT_ROOT/public/version.txt"
if [[ ! -f "$VERSION_FILE" ]]; then
    printf '%bError: Version file not found at %s%b\n' "$RED" "$VERSION_FILE" "$NC" >&2
    exit 1
fi

VERSION="$(tr -d '[:space:]' < "$VERSION_FILE")"
printf '%bVersion: %s%b\n\n' "$GREEN" "$VERSION" "$NC"

printf '%bSyncing application version...%b\n' "$YELLOW" "$NC"
pnpm run sync-version

if [[ "$SKIP_RUNNER_BUILD" == false ]]; then
    printf '\n%b[1/3] Building sidecar binaries...%b\n' "$YELLOW" "$NC"
    pnpm run build:runner
    pnpm run build:cdp-launcher
    printf '%b  Sidecar builds complete.%b\n' "$GREEN" "$NC"
else
    printf '\n%b[1/3] Skipping sidecar builds (--skip-runner-build)%b\n' "$GRAY" "$NC"
fi

if [[ "$SKIP_TAURI_BUILD" == false ]]; then
    printf '\n%b[2/3] Building Tauri application and Linux bundles...%b\n' "$YELLOW" "$NC"
    pnpm tauri build --bundles deb,appimage
    printf '%b  Tauri build complete.%b\n' "$GREEN" "$NC"
else
    printf '\n%b[2/3] Skipping Tauri build (--skip-tauri-build)%b\n' "$GRAY" "$NC"
fi

printf '\n%b[3/3] Build summary%b\n' "$YELLOW" "$NC"
BUNDLE_DIR="$PROJECT_ROOT/target/release/bundle"
artifact_found=false

if [[ -d "$BUNDLE_DIR" ]]; then
    while IFS= read -r artifact; do
        artifact_found=true
        artifact_size="$(du -h "$artifact" | cut -f1)"
        printf '%b%s%b (%s)\n' "$WHITE" "$artifact" "$NC" "$artifact_size"
    done < <(find "$BUNDLE_DIR" -maxdepth 2 -type f \
        \( -name '*.deb' -o -name '*.AppImage' \) | sort)
fi

if [[ "$artifact_found" == false && "$SKIP_TAURI_BUILD" == false ]]; then
    printf '%bNo Linux bundle artifacts were found under %s.%b\n' "$YELLOW" "$BUNDLE_DIR" "$NC"
fi

printf '\n%bBuild complete.%b\n' "$GREEN" "$NC"
printf '%bNote: Automatic local Discord token extraction is not yet supported on Linux; use CDP login.%b\n' "$YELLOW" "$NC"
