#!/bin/bash
# Discord Quest Helper - macOS Build Script
# This script builds and packages the application for macOS

set -e

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
    echo -e "${YELLOW}[1/4] Building src-runner...${NC}"
    pnpm run build:runner
    echo -e "${GREEN}  src-runner build complete.${NC}"
else
    echo -e "${GRAY}[1/4] Skipping src-runner build (--skip-runner-build)${NC}"
fi

# Copy runner to data directory for development
RUNNER_DST="$SRC_TAURI/data/discord-quest-runner"
if [ -f "$RUNNER_DST" ]; then
    chmod +x "$RUNNER_DST"
fi

# Step 2: Build the CDP launcher sidecar required by the Tauri bundle
if [ "$SKIP_TAURI_BUILD" = false ]; then
    echo -e "${YELLOW}[2/4] Building CDP launcher sidecar...${NC}"
    pnpm run build:cdp-launcher
    echo -e "${GREEN}  CDP launcher sidecar build complete.${NC}"
else
    echo -e "${GRAY}[2/4] Skipping CDP launcher sidecar build (--skip-tauri-build)${NC}"
fi

# Step 3: Build Tauri app
if [ "$SKIP_TAURI_BUILD" = false ]; then
    echo -e "${YELLOW}[3/4] Building Tauri application...${NC}"
    pnpm tauri build
    echo -e "${GREEN}  Tauri build complete.${NC}"
else
    echo -e "${GRAY}[3/4] Skipping Tauri build (--skip-tauri-build)${NC}"
fi

# Step 4: Summary
echo ""
echo -e "${CYAN}========================================${NC}"
echo -e "${GREEN}  Build Complete!${NC}"
echo -e "${CYAN}========================================${NC}"
echo ""

# Find and display the built app/dmg
DMG_FILE=$(find "$RELEASE_DIR/bundle/dmg" -name "*.dmg" 2>/dev/null | head -1)
APP_FILE=$(find "$RELEASE_DIR/bundle/macos" -name "*.app" -type d 2>/dev/null | head -1)

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
