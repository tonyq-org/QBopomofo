#!/bin/bash
# QBopomofo Data Provider — Build Script
#
# Compiles Chewing CSV + custom data into optimized binary Trie format
# using the chewing-cli tool from base/engine/tools.
#
# Usage: ./data-provider/build.sh
#
# Output: data-provider/output/word.dat, tsi.dat, symbols.dat, swkb.dat

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ENGINE_DIR="$PROJECT_ROOT/base/engine"
DATA_DIR="$SCRIPT_DIR/chewing-data"
CUSTOM_DIR="$SCRIPT_DIR/custom-data"
OUTPUT_DIR="$SCRIPT_DIR/output"

echo "=== QBopomofo Data Provider ==="
echo "Building dictionary data from CSV..."
echo ""

# Build chewing-cli tool
echo "[1/5] Building chewing-cli..."
cargo build --release --manifest-path "$ENGINE_DIR/tools/Cargo.toml"

CLI="$ENGINE_DIR/target/release/chewing-cli"

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Merge upstream tsi.csv with custom phrases
echo "[2/5] Merging custom phrases..."
MERGED_TSI="$OUTPUT_DIR/_merged_tsi.csv"
cp "$DATA_DIR/tsi.csv" "$MERGED_TSI"
# Append all custom CSV files (skip empty or comment-only files)
for csv in "$CUSTOM_DIR"/*.csv; do
  if [ -f "$csv" ]; then
    LINES=$(grep -cv '^\s*#\|^\s*$' "$csv" 2>/dev/null || true)
    if [ "$LINES" -gt 0 ]; then
      echo "  + $(basename "$csv") ($LINES entries)"
      grep -v '^\s*#\|^\s*$' "$csv" >> "$MERGED_TSI"
    fi
  fi
done

# Enrich word.csv with single-char frequencies from tsi.csv
echo "[3/6] Enriching word.csv with tsi.csv single-char frequencies..."
ENRICHED_WORD="$OUTPUT_DIR/_enriched_word.csv"
python3 -c "
import csv, sys

# Read single-char frequencies from tsi.csv (pick max freq per char+zhuyin)
freq = {}
with open('$MERGED_TSI') as f:
    for row in csv.reader(f):
        if not row or row[0].startswith('#') or len(row) < 3:
            continue
        word, fr, zhuyin = row[0], row[1], row[2]
        if len(word) == 1:
            key = (word, zhuyin)
            freq[key] = max(freq.get(key, 0), int(fr))

# Enrich word.csv: replace freq=0 with tsi freq if available
updated = 0
with open('$DATA_DIR/word.csv') as fin, open('$ENRICHED_WORD', 'w', newline='') as fout:
    writer = csv.writer(fout)
    for row in csv.reader(fin):
        if not row or row[0].startswith('#'):
            fout.write(','.join(row) + '\n')
            continue
        if len(row) >= 3:
            word, fr, zhuyin = row[0], int(row[1]), row[2]
            key = (word, zhuyin)
            if fr == 0 and key in freq:
                row[1] = str(freq[key])
                updated += 1
        writer.writerow(row)

print(f'  Enriched {updated} single-char entries with tsi.csv frequencies')
"

# Build word.dat (single character dictionary)
echo "[4/6] Building word.dat from enriched word.csv..."
"$CLI" init-database \
  --db-type trie \
  --csv \
  --skip-invalid \
  "$ENRICHED_WORD" \
  "$OUTPUT_DIR/word.dat"
rm -f "$ENRICHED_WORD"

echo ""

# Build tsi.dat (phrase dictionary — merged with custom data)
echo "[5/6] Building tsi.dat from merged tsi.csv + custom phrases..."
"$CLI" init-database \
  --db-type trie \
  --csv \
  --skip-invalid \
  "$MERGED_TSI" \
  "$OUTPUT_DIR/tsi.dat"

# Clean up temp file
rm -f "$MERGED_TSI"

echo ""

# Copy symbol and abbreviation tables (raw text, no compilation needed)
echo "[6/6] Copying symbols.dat and swkb.dat..."
cp "$DATA_DIR/symbols.dat" "$OUTPUT_DIR/symbols.dat"
cp "$DATA_DIR/swkb.dat" "$OUTPUT_DIR/swkb.dat"

echo ""
echo "=== Done ==="
echo "Output files:"
ls -lh "$OUTPUT_DIR"/*.dat
