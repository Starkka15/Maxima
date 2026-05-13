#!/bin/bash
# Maxima Build Script — Cross-compile for Windows + Generate Installer
# Run from project root on macOS
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TARGET="x86_64-pc-windows-gnu"
RELEASE_DIR="$PROJECT_ROOT/target/$TARGET/release"

echo "=== Maxima Build System ==="
echo "Platform: macOS → Windows (cross-compile)"
echo "Target:   $TARGET"
echo ""

# ---- Step 1: Verify Prerequisites ----
echo "[1/4] Verifying prerequisites..."

if ! command -v x86_64-w64-mingw32-gcc &> /dev/null; then
    echo "ERROR: MinGW not found. Install with: brew install mingw-w64"
    exit 1
fi

if ! command -v makensis &> /dev/null; then
    echo "ERROR: NSIS not found. Install with: brew install nsis"
    exit 1
fi

if ! rustup target list --installed --toolchain nightly 2>/dev/null | grep -q "$TARGET"; then
    echo "Adding Rust target $TARGET..."
    rustup target add --toolchain nightly "$TARGET"
fi

echo "  ✓ MinGW-w64: $(x86_64-w64-mingw32-gcc --version | head -1)"
echo "  ✓ NSIS:      $(makensis -VERSION 2>/dev/null || echo 'installed')"
echo "  ✓ Target:    $TARGET"
echo ""

# ---- Step 2: Cross-Compile ----
echo "[2/4] Cross-compiling Maxima for Windows..."
cd "$PROJECT_ROOT"

cargo +nightly build --release --target "$TARGET" 2>&1

echo ""
echo "  Binaries:"
for bin in maxima-bootstrap maxima-cli maxima-service maxima-tui maxima; do
    if [ -f "$RELEASE_DIR/$bin.exe" ]; then
        SIZE=$(du -h "$RELEASE_DIR/$bin.exe" | cut -f1)
        echo "    ✓ $bin.exe ($SIZE)"
    else
        echo "    ✗ $bin.exe (not found — may be expected)"
    fi
done
echo ""

# ---- Step 3: Verify core binaries exist ----
echo "[3/4] Verifying core binaries..."
MISSING=0
for bin in maxima-bootstrap maxima-cli maxima-service; do
    if [ ! -f "$RELEASE_DIR/$bin.exe" ]; then
        echo "  ERROR: Required binary $bin.exe not found!"
        MISSING=1
    fi
done

if [ "$MISSING" -eq 1 ]; then
    echo "Build failed — missing required binaries."
    exit 1
fi
echo "  ✓ All core binaries present"
echo ""

# ---- Step 4: Build Installer ----
echo "[4/4] Building Windows installer..."
cd "$SCRIPT_DIR"

makensis maxima-setup.nsi

echo ""
echo "=== Build Complete ==="
if [ -f "$SCRIPT_DIR/MaximaSetup.exe" ]; then
    SIZE=$(du -h "$SCRIPT_DIR/MaximaSetup.exe" | cut -f1)
    echo "  Installer: $SCRIPT_DIR/MaximaSetup.exe ($SIZE)"
else
    echo "  ERROR: Installer was not created!"
    exit 1
fi
