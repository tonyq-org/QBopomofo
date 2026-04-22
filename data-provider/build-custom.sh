#!/usr/bin/env bash
# Builds custom.dat from phrases.csv only (no main tsi.csv).
# Fast path for hot-reload iteration — skips Rust engine recompile.
#
# Output: data-provider/output/custom.dat

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ENGINE_DIR="$SCRIPT_DIR/../base/engine"
CUSTOM_CSV="$SCRIPT_DIR/custom-data/phrases.csv"
OUTPUT_DIR="$SCRIPT_DIR/output"
OUTPUT="$OUTPUT_DIR/custom.dat"

mkdir -p "$OUTPUT_DIR"

# Build chewing-cli if not already built
CLI="$ENGINE_DIR/target/release/chewing-cli"
if [ ! -f "$CLI" ]; then
    echo "Building chewing-cli..."
    cargo build --release --manifest-path "$ENGINE_DIR/tools/Cargo.toml"
fi

# Strip comment/blank lines before passing to CLI
FILTERED="$OUTPUT_DIR/_custom_filtered.csv"
grep -v '^\s*#\|^\s*$' "$CUSTOM_CSV" > "$FILTERED"

LINES=$(wc -l < "$FILTERED" | tr -d ' ')
echo "Building custom.dat ($LINES entries)..."
"$CLI" init-database --db-type trie --csv --skip-invalid "$FILTERED" "$OUTPUT"
rm -f "$FILTERED"

echo "Done: $OUTPUT"
