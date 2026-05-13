#!/bin/bash
# MaximaHelper build script
# Compiles the native Swift helper app and registers the qrc:// URL scheme.
# Run once from your Mac (outside CrossOver) before launching Maxima inside a bottle.
#
# Usage: bash MaximaHelper/build.sh [--output <dir>]
#   --output <dir>   Where to place MaximaHelper.app  (default: repo root)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUTPUT_DIR="${SCRIPT_DIR}/.."

# Parse optional --output argument
while [[ $# -gt 0 ]]; do
    case "$1" in
        --output) OUTPUT_DIR="$2"; shift 2 ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

APP_NAME="MaximaHelper"
APP_BUNDLE="${OUTPUT_DIR}/${APP_NAME}.app"
BUILD_DIR="${SCRIPT_DIR}/build"

echo "=== MaximaHelper Build ==="
echo "Output: ${APP_BUNDLE}"
echo ""

# ---- Prerequisites ----
if ! command -v swiftc &>/dev/null; then
    echo "ERROR: swiftc not found. Install Xcode Command Line Tools:"
    echo "  xcode-select --install"
    exit 1
fi

echo "[1/4] Cleaning previous build..."
rm -rf "${BUILD_DIR}" "${APP_BUNDLE}"
mkdir -p "${BUILD_DIR}"

echo "[2/4] Compiling Swift source (universal binary)..."
SDK="$(xcrun --show-sdk-path)"

swiftc -O -sdk "${SDK}" -target arm64-apple-macos12.0 \
    -framework Cocoa -framework Foundation \
    "${SCRIPT_DIR}/Sources/main.swift" \
    -o "${BUILD_DIR}/${APP_NAME}_arm64"

swiftc -O -sdk "${SDK}" -target x86_64-apple-macos12.0 \
    -framework Cocoa -framework Foundation \
    "${SCRIPT_DIR}/Sources/main.swift" \
    -o "${BUILD_DIR}/${APP_NAME}_x86_64"

lipo -create \
    "${BUILD_DIR}/${APP_NAME}_arm64" \
    "${BUILD_DIR}/${APP_NAME}_x86_64" \
    -output "${BUILD_DIR}/${APP_NAME}"

echo "[3/4] Assembling app bundle..."
mkdir -p "${APP_BUNDLE}/Contents/MacOS"
cp "${BUILD_DIR}/${APP_NAME}"        "${APP_BUNDLE}/Contents/MacOS/${APP_NAME}"
cp "${SCRIPT_DIR}/Info.plist"        "${APP_BUNDLE}/Contents/Info.plist"
printf 'APPL????' > "${APP_BUNDLE}/Contents/PkgInfo"
chmod +x "${APP_BUNDLE}/Contents/MacOS/${APP_NAME}"

echo "[4/4] Registering qrc:// protocol with macOS..."
LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister"
"${LSREGISTER}" -f "${APP_BUNDLE}"

echo ""
echo "=== Done ==="
echo "MaximaHelper.app is ready at: ${APP_BUNDLE}"
echo ""
echo "When Maxima's login opens a browser and gets stuck on a qrc:// redirect,"
echo "macOS will silently forward it to Maxima's listener on port 31033."
echo ""
echo "Keep MaximaHelper.app in a stable location — moving it requires re-running this script."
