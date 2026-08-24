#!/bin/bash
# Discord Quest Helper - macOS Build Script
# This script builds and packages the application for macOS

set -Eeuo pipefail

# Get script directory (project root)
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_ROOT"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
WHITE='\033[1;37m'
GRAY='\033[0;90m'
NC='\033[0m' # No Color

echo -e "${CYAN}========================================${NC}"
echo -e "${CYAN}  Discord Quest Helper Build Script${NC}"
echo -e "${CYAN}  (macOS)${NC}"
echo -e "${CYAN}========================================${NC}"
echo ""

# Parse arguments
SKIP_RUNNER_BUILD=false
SKIP_TAURI_BUILD=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-runner-build)
            SKIP_RUNNER_BUILD=true
            shift
            ;;
        --skip-tauri-build)
            SKIP_TAURI_BUILD=true
            shift
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            exit 1
            ;;
    esac
done

# Read version from public/version.txt
VERSION_FILE="$PROJECT_ROOT/public/version.txt"
if [ ! -f "$VERSION_FILE" ]; then
    echo -e "${RED}Error: Version file not found at $VERSION_FILE${NC}"
    exit 1
fi
VERSION=$(cat "$VERSION_FILE" | tr -d '[:space:]')
echo -e "${GREEN}Version: $VERSION${NC}"
echo ""

# Define paths
SRC_TAURI="$PROJECT_ROOT/src-tauri"
RELEASE_DIR="$PROJECT_ROOT/target/release"

# Step 1: Build src-runner
if [ "$SKIP_RUNNER_BUILD" = false ]; then
    echo -e "${YELLOW}[1/5] Building src-runner...${NC}"
    pnpm run build:runner
    echo -e "${GREEN}  src-runner build complete.${NC}"
else
    echo -e "${GRAY}[1/5] Skipping src-runner build (--skip-runner-build)${NC}"
fi

# Copy runner to data directory for development
RUNNER_DST="$SRC_TAURI/data/stagecraft"
if [ -f "$RUNNER_DST" ]; then
    chmod +x "$RUNNER_DST"
fi

# Step 2: Build the CDP launcher sidecar required by the Tauri bundle
if [ "$SKIP_TAURI_BUILD" = false ]; then
    echo -e "${YELLOW}[2/5] Building CDP launcher sidecar...${NC}"
    pnpm run build:cdp-launcher
    echo -e "${GREEN}  CDP launcher sidecar build complete.${NC}"
else
    echo -e "${GRAY}[2/5] Skipping CDP launcher sidecar build (--skip-tauri-build)${NC}"
fi

# Step 3: Build Tauri app
if [ "$SKIP_TAURI_BUILD" = false ]; then
    TARGET_TRIPLE="${TAURI_TARGET_TRIPLE:-$(rustc -vV | awk '/^host:/ { print $2 }')}"
    BRIDGE_PATH="$SRC_TAURI/binaries/waybridge-$TARGET_TRIPLE"
    RUNNER_PATH="$SRC_TAURI/data/stagecraft"
    echo -e "${YELLOW}[3/5] Signing runtime payloads before bundling...${NC}"
    runtime_payloads=("$BRIDGE_PATH")
    if [ -f "$RUNNER_PATH" ]; then
        runtime_payloads+=("$RUNNER_PATH")
    elif [ "$SKIP_RUNNER_BUILD" = false ]; then
        echo -e "${RED}Runner payload was not produced: $RUNNER_PATH${NC}" >&2
        exit 1
    fi
    "$PROJECT_ROOT/scripts/sign-macos-runtime.sh" "${runtime_payloads[@]}"

    echo -e "${YELLOW}[4/5] Building Tauri application...${NC}"
    pnpm tauri build
    echo -e "${GREEN}  Tauri build complete.${NC}"
else
    echo -e "${GRAY}[3/5] Skipping signing and Tauri build (--skip-tauri-build)${NC}"
fi

# Step 5: Verify and archive symbols
APP_FILE=$(find "$RELEASE_DIR/bundle/macos" -name "*.app" -type d 2>/dev/null | head -1 || true)
DMG_FILE=$(find "$RELEASE_DIR/bundle/dmg" -name "*.dmg" 2>/dev/null | head -1 || true)
if [ "$SKIP_TAURI_BUILD" = false ]; then
    if [ -z "$APP_FILE" ]; then
        echo -e "${RED}No macOS app bundle was produced.${NC}" >&2
        exit 1
    fi
    echo -e "${YELLOW}[5/5] Verifying bundle identity and signatures...${NC}"
    if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
        "$PROJECT_ROOT/scripts/sign-macos-bundle-smoke.sh" "$APP_FILE"
    fi
    "$PROJECT_ROOT/scripts/verify-macos-bundle.sh" "$APP_FILE"
    audit_args=(--platform macos --artifact "$APP_FILE" --output "$RELEASE_DIR/identity-manifest.json")
    if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
        audit_args+=(--allow-adhoc)
    fi
    node "$PROJECT_ROOT/scripts/audit-packaged-identity.mjs" "${audit_args[@]}"

    if [ "${REQUIRE_NOTARIZATION:-0}" = "1" ]; then
        if [ -z "$DMG_FILE" ]; then
            echo -e "${RED}Notarization is required but no DMG was produced.${NC}" >&2
            exit 1
        fi
        codesign --verify --verbose=4 "$DMG_FILE"
        xcrun stapler validate "$DMG_FILE"
    fi

    SYMBOL_ARCHIVE="$RELEASE_DIR/discord-quest-helper-macos-symbols.zip"
    symbol_paths=()
    while IFS= read -r path; do symbol_paths+=("$path"); done < <(
        find "$PROJECT_ROOT/target" -type d \
            \( -name 'meridian*.dSYM' -o -name 'discord_quest_helper-*.dSYM' \
               -o -name 'waybridge*.dSYM' -o -name 'stagecraft*.dSYM' \) \
            -prune
    )
    if [ ${#symbol_paths[@]} -gt 0 ]; then
        SYMBOL_STAGE="$(mktemp -d)"
        trap 'rm -rf "$SYMBOL_STAGE"' EXIT
        for symbol_path in "${symbol_paths[@]}"; do
            ditto "$symbol_path" "$SYMBOL_STAGE/$(basename "$symbol_path")"
        done
        ditto -c -k --sequesterRsrc "$SYMBOL_STAGE" "$SYMBOL_ARCHIVE"
        rm -rf "$SYMBOL_STAGE"
        trap - EXIT
        echo -e "${GREEN}  Symbols: $SYMBOL_ARCHIVE${NC}"
    else
        echo -e "${YELLOW}  No dSYM bundles were found to archive.${NC}"
    fi
fi

# Summary
echo ""
echo -e "${CYAN}========================================${NC}"
echo -e "${GREEN}  Build Complete!${NC}"
echo -e "${CYAN}========================================${NC}"
echo ""

if [ -n "$DMG_FILE" ]; then
    DMG_SIZE=$(du -h "$DMG_FILE" | cut -f1)
    echo -e "${WHITE}DMG: $DMG_FILE${NC}"
    echo -e "${WHITE}Size: $DMG_SIZE${NC}"
fi

if [ -n "$APP_FILE" ]; then
    APP_SIZE=$(du -sh "$APP_FILE" | cut -f1)
    echo -e "${WHITE}App: $APP_FILE${NC}"
    echo -e "${WHITE}Size: $APP_SIZE${NC}"
fi

echo ""
echo -e "${GREEN}Done!${NC}"
