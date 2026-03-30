#!/bin/bash
# QBopomofo Data Provider — Build Script
#
# Compiles Chewing CSV data into optimized binary Trie format
# using the chewing-cli tool from base/engine/tools.
#
# Usage: ./data-provider/build.sh
#
# Output: data-provider/output/word.dat, tsi.dat

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ENGINE_DIR="$PROJECT_ROOT/base/engine"
DATA_DIR="$SCRIPT_DIR/chewing-data"
OUTPUT_DIR="$SCRIPT_DIR/output"

echo "=== QBopomofo Data Provider ==="
echo "Building dictionary data from CSV..."
echo ""

# Build chewing-cli tool
echo "[1/3] Building chewing-cli..."
cargo build --release --manifest-path "$ENGINE_DIR/tools/Cargo.toml"

CLI="$ENGINE_DIR/target/release/chewing-cli"

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Build word.dat (single character dictionary)
echo "[2/3] Building word.dat from word.csv..."
"$CLI" init-database \
  --db-type trie \
  --csv \
  --skip-invalid \
  "$DATA_DIR/word.csv" \
  "$OUTPUT_DIR/word.dat"

echo ""

# Build tsi.dat (phrase dictionary)
echo "[3/4] Building tsi.dat from tsi.csv..."
"$CLI" init-database \
  --db-type trie \
  --csv \
  --skip-invalid \
  "$DATA_DIR/tsi.csv" \
  "$OUTPUT_DIR/tsi.dat"

echo ""

# Copy symbol and abbreviation tables (raw text, no compilation needed)
echo "[4/4] Copying symbols.dat and swkb.dat..."
cp "$DATA_DIR/symbols.dat" "$OUTPUT_DIR/symbols.dat"
cp "$DATA_DIR/swkb.dat" "$OUTPUT_DIR/swkb.dat"

echo ""
echo "=== Done ==="
echo "Output files:"
ls -lh "$OUTPUT_DIR"/*
