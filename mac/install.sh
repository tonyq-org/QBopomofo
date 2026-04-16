#!/bin/bash
# Build and install QBopomofo input method locally
# Usage: ./install.sh [--clean] [--debug]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
APP_NAME="QBopomofo"
INSTALL_DIR="$HOME/Library/Input Methods"
INSTALL_PATH="$INSTALL_DIR/$APP_NAME.app"

# Parse args
CLEAN_ARG=""
DEBUG=0
for arg in "$@"; do
    case "$arg" in
        --clean) CLEAN_ARG="--clean" ;;
        --debug) DEBUG=1 ;;
        *) echo "Unknown arg: $arg"; exit 1 ;;
    esac
done

# Step 0: Rebuild dictionary data (ensures custom phrases are included)
echo "→ Building dictionary data..."
bash "$SCRIPT_DIR/../data-provider/build.sh"

# Step 1: Build
bash "$SCRIPT_DIR/build-app.sh" $CLEAN_ARG

APP_BUNDLE="$SCRIPT_DIR/.build/$APP_NAME.app"

# Step 2: Kill ALL existing QBopomofo processes (including macOS auto-launched ones)
echo "→ Stopping all QBopomofo processes..."
pkill -9 -f "QBopomofo.app/Contents/MacOS/QBopomofo" 2>/dev/null || true
sleep 1
# Kill again in case macOS restarted it
pkill -9 -f "QBopomofo.app/Contents/MacOS/QBopomofo" 2>/dev/null || true
sleep 1

# Step 3: Install to ~/Library/Input Methods/
echo "→ Installing to $INSTALL_DIR/..."
mkdir -p "$INSTALL_DIR"
rm -rf "$INSTALL_PATH"
cp -R "$APP_BUNDLE" "$INSTALL_PATH"

# Step 4: Register input source
echo "→ Registering input source..."
"$INSTALL_PATH/Contents/MacOS/$APP_NAME" install

# Step 5: Launch — we are the only instance
if [ "$DEBUG" -eq 1 ]; then
    echo "→ Launching in debug mode (log: /tmp/qbopomofo.log)..."
    QBOPOMOFO_DEBUG=1 "$INSTALL_PATH/Contents/MacOS/$APP_NAME" >> /tmp/qbopomofo.log 2>&1 &
    echo "  PID: $!"
    echo "  tail -f /tmp/qbopomofo.log"
else
    echo "→ Launching..."
    "$INSTALL_PATH/Contents/MacOS/$APP_NAME" &
fi

echo ""
echo "=== Installed ==="
echo "如果是首次安裝，請到："
echo "  系統設定 → 鍵盤 → 輸入方式 → + → Q注音"
