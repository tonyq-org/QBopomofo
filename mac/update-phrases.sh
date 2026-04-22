#!/usr/bin/env bash
# Hot-reload custom phrases without reinstalling.
#
# Usage: ./mac/update-phrases.sh
#
# 1. Rebuilds custom.dat from data-provider/custom-data/phrases.csv
# 2. Copies it to ~/Library/Application Support/QBopomofo/
# 3. Sends SIGUSR1 to the running input method to reload immediately

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

source "$HOME/.cargo/env" 2>/dev/null || true

"$REPO_ROOT/data-provider/build-custom.sh"

DEST="$HOME/Library/Application Support/QBopomofo"
mkdir -p "$DEST"
cp "$REPO_ROOT/data-provider/output/custom.dat" "$DEST/custom.dat"
echo "Copied → $DEST/custom.dat"

PID=$(pgrep -x QBopomofo 2>/dev/null | head -1 || true)
if [ -n "$PID" ]; then
    kill -USR1 "$PID"
    echo "Reloaded (SIGUSR1 → PID $PID)"
else
    echo "QBopomofo not running — custom.dat will load on next start"
fi
