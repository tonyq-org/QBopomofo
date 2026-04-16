#!/bin/bash
# Build a distributable .pkg installer for QBopomofo.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BUILD_DIR="$SCRIPT_DIR/.build"
APP_NAME="QBopomofo"
APP_BUNDLE="$BUILD_DIR/$APP_NAME.app"
PKG_ROOT="$BUILD_DIR/pkgroot"
SCRIPTS_DIR="$SCRIPT_DIR/pkg"
OUTPUT_PKG="$BUILD_DIR/$APP_NAME-Installer.pkg"
IDENTIFIER="org.qbopomofo.inputmethod.installer"
VERSION="0.1.0"

if ! command -v pkgbuild >/dev/null 2>&1; then
    echo "ERROR: pkgbuild not found. Install Xcode Command Line Tools first."
    exit 1
fi

bash "$SCRIPT_DIR/build-app.sh"

echo "→ Preparing pkg payload..."
rm -rf "$PKG_ROOT" "$OUTPUT_PKG"
mkdir -p "$PKG_ROOT/Library/Input Methods"
cp -R "$APP_BUNDLE" "$PKG_ROOT/Library/Input Methods/$APP_NAME.app"

echo "→ Building installer package..."
pkgbuild \
    --root "$PKG_ROOT" \
    --identifier "$IDENTIFIER" \
    --version "$VERSION" \
    --install-location "/" \
    --scripts "$SCRIPTS_DIR" \
    "$OUTPUT_PKG"

echo ""
echo "=== PKG Build complete ==="
echo "Installer: $OUTPUT_PKG"
echo ""
echo "On the target Mac:"
echo "  1. Open the .pkg"
echo "  2. Complete the installer"
echo "  3. Go to System Settings → Keyboard → Input Sources → + → Q注音"
