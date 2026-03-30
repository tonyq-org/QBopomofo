#!/bin/bash
# Build and run QBopomofo TestApp
# Usage: ./run.sh [--clean]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ENGINE_DIR="$(cd "$SCRIPT_DIR/../../base/engine" && pwd)"
DATA_DIR="$(cd "$SCRIPT_DIR/../../data-provider" && pwd)/output"

# Parse args
CLEAN=0
for arg in "$@"; do
    case "$arg" in
        --clean) CLEAN=1 ;;
        *) echo "Unknown arg: $arg"; exit 1 ;;
    esac
done

echo "=== QBopomofo TestApp ==="

# Step 1: Build dictionary data if needed
if [ ! -f "$DATA_DIR/word.dat" ]; then
    echo "→ Building dictionary data..."
    cd "$SCRIPT_DIR/../../data-provider"
    bash build.sh
fi

# Step 2: Build Rust engine (capi)
echo "→ Building Rust engine..."
cd "$ENGINE_DIR/capi"
cargo build --release 2>&1 | grep -v "^$" | tail -3
echo "  libchewing_capi.a updated: $(stat -f '%Sm' "$ENGINE_DIR/target/release/libchewing_capi.a")"

# Step 3: Build Swift TestApp
cd "$SCRIPT_DIR"
if [ "$CLEAN" -eq 1 ]; then
    echo "→ Clean build..."
    swift package clean 2>/dev/null || true
fi
echo "→ Building Swift TestApp..."
swift build 2>&1 | tail -5

# Step 4: Kill existing instance if running
if pgrep -f QBopomofoTestApp > /dev/null 2>&1; then
    echo "→ Stopping existing TestApp..."
    pkill -f QBopomofoTestApp || true
    sleep 0.5
fi

# Step 5: Launch
echo "→ Launching TestApp..."
BINARY="$(swift build --show-bin-path)/QBopomofoTestApp"
"$BINARY" &
echo "  PID: $!"
echo "=== Ready ==="
