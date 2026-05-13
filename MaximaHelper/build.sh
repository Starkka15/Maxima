#!/bin/bash
# MaximaHelper build script
# Compiles the native Swift helper app and registers the qrc:// URL scheme.
# Run once from your Mac (outside CrossOver) before launching Maxima inside a bottle.
#
# Usage: bash MaximaHelper/build.sh [--output <dir>] [--no-register]
#   --output <dir>    Where to place MaximaHelper.app  (default: repo root)
#   --no-register     Skip lsregister (useful in CI)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
OUTPUT_DIR="${PROJECT_ROOT}"
REGISTER=true

while [[ $# -gt 0 ]]; do
    case "$1" in
        --output)      OUTPUT_DIR="$2"; shift 2 ;;
        --no-register) REGISTER=false; shift ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

APP_NAME="MaximaHelper"
APP_BUNDLE="${OUTPUT_DIR}/${APP_NAME}.app"
BUILD_DIR="${SCRIPT_DIR}/build"
LOGO="${PROJECT_ROOT}/maxima-resources/assets/logo.png"

echo "=== MaximaHelper Build ==="
echo "Output: ${APP_BUNDLE}"
echo ""

if ! command -v swiftc &>/dev/null; then
    echo "ERROR: swiftc not found. Install Xcode Command Line Tools:"
    echo "  xcode-select --install"
    exit 1
fi

echo "[1/5] Cleaning previous build..."
rm -rf "${BUILD_DIR}" "${APP_BUNDLE}"
mkdir -p "${BUILD_DIR}"

echo "[2/5] Compiling Swift source (universal binary)..."
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

echo "[3/5] Generating app icon..."
RESOURCES_DIR="${APP_BUNDLE}/Contents/Resources"
mkdir -p "${RESOURCES_DIR}"

if [ -f "${LOGO}" ] && command -v iconutil &>/dev/null; then
    ICONSET="${BUILD_DIR}/AppIcon.iconset"
    mkdir -p "${ICONSET}"
    for size in 16 32 128 256 512; do
        double=$((size * 2))
        sips -z "${size}" "${size}"     "${LOGO}" --out "${ICONSET}/icon_${size}x${size}.png"    >/dev/null 2>&1
        sips -z "${double}" "${double}" "${LOGO}" --out "${ICONSET}/icon_${size}x${size}@2x.png" >/dev/null 2>&1
    done
    iconutil -c icns "${ICONSET}" -o "${RESOURCES_DIR}/AppIcon.icns"
    echo "  ✓ AppIcon.icns"
else
    echo "  ⚠ Skipped (logo.png or iconutil not available)"
fi

echo "[4/5] Assembling app bundle..."
mkdir -p "${APP_BUNDLE}/Contents/MacOS"
cp "${BUILD_DIR}/${APP_NAME}" "${APP_BUNDLE}/Contents/MacOS/${APP_NAME}"
cp "${SCRIPT_DIR}/Info.plist" "${APP_BUNDLE}/Contents/Info.plist"
printf 'APPL????' > "${APP_BUNDLE}/Contents/PkgInfo"
chmod +x "${APP_BUNDLE}/Contents/MacOS/${APP_NAME}"

echo "[5/5] Registering qrc:// protocol with macOS..."
if [ "${REGISTER}" = true ]; then
    LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister"
    "${LSREGISTER}" -f "${APP_BUNDLE}"
    echo "  ✓ Registered"
else
    echo "  Skipped (--no-register)"
fi

echo ""
echo "=== Done ==="
echo "MaximaHelper.app → ${APP_BUNDLE}"
