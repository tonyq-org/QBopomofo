#!/bin/bash
# Build QBopomofo.app input method bundle
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_DIR="$SCRIPT_DIR/.build"
APP_NAME="QBopomofo"
APP_BUNDLE="$BUILD_DIR/$APP_NAME.app"
DATA_DIR="$PROJECT_ROOT/data-provider/output"

echo "=== Building QBopomofo Input Method ==="

# Step 1: Build data if needed
if [ ! -f "$DATA_DIR/tsi.dat" ]; then
    echo "→ Building dictionary data..."
    cd "$PROJECT_ROOT/data-provider"
    bash build.sh
fi

# Step 2: Build release binary
echo "→ Building release binary..."
cd "$SCRIPT_DIR"
swift build -c release 2>&1

BINARY="$(swift build -c release --show-bin-path)/QBopomofo"
if [ ! -f "$BINARY" ]; then
    echo "ERROR: Binary not found at $BINARY"
    exit 1
fi

# Step 3: Assemble .app bundle
echo "→ Assembling $APP_NAME.app..."
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

# Binary
cp "$BINARY" "$APP_BUNDLE/Contents/MacOS/$APP_NAME"

# Info.plist
cp "$SCRIPT_DIR/Info.plist" "$APP_BUNDLE/Contents/Info.plist"

# PkgInfo
echo -n "APPL????" > "$APP_BUNDLE/Contents/PkgInfo"

# Dictionary data
cp "$DATA_DIR/word.dat" "$APP_BUNDLE/Contents/Resources/"
cp "$DATA_DIR/tsi.dat" "$APP_BUNDLE/Contents/Resources/"
cp "$DATA_DIR/symbols.dat" "$APP_BUNDLE/Contents/Resources/"
cp "$DATA_DIR/swkb.dat" "$APP_BUNDLE/Contents/Resources/"

# Step 4: Ad-hoc code sign
echo "→ Code signing..."
codesign --deep --force --sign - "$APP_BUNDLE" 2>/dev/null || {
    echo "WARNING: Code signing failed (may need Xcode command line tools)"
}

echo ""
echo "=== Build complete ==="
echo "Bundle: $APP_BUNDLE"
echo ""
echo "To install:"
echo "  cp -R $APP_BUNDLE ~/Library/Input\\ Methods/"
echo "  $APP_BUNDLE/Contents/MacOS/$APP_NAME install"
echo ""
echo "Then go to System Settings → Keyboard → Input Sources → + → QBopomofo"
echo "You may need to log out and log back in."
