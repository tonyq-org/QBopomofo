#!/usr/bin/env python3
"""
Extract phrases from g0v/moedict-data (教育部重編國語辭典 修訂本) that are
not already present in our dictionary, and emit them with freq=1.

Source dataset:
  https://github.com/g0v/moedict-data
  Original: 教育部《重編國語辭典（修訂本）》
  Original license: CC BY-ND 3.0 TW (姓名標示-禁止改作 臺灣 3.0)
  Note: Per MOE clarification, the ND restriction applies to modification of
  the textual content itself; format conversion and reuse of headword lists
  are permitted. We extract only headword + bopomofo (no definitions).

Output: data-provider/custom-data/moedict-phrases.csv
"""
from __future__ import annotations

import json
import sys
import urllib.request
import lzma
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TSI = ROOT / "chewing-data" / "tsi.csv"
WORD = ROOT / "chewing-data" / "word.csv"
CUSTOM = ROOT / "custom-data" / "phrases.csv"
OUTPUT = ROOT / "custom-data" / "moedict-phrases.csv"

CACHE_DIR = Path("/tmp/moedict")
CACHE_XZ = CACHE_DIR / "dict-revised.json.xz"
CACHE_JSON = CACHE_DIR / "dict-revised.json"
SOURCE_URL = "https://github.com/g0v/moedict-data/raw/master/dict-revised.json.xz"


def load_existing_pairs() -> set[tuple[str, str]]:
    """Build dedupe set of (word, normalized_bopomofo) from existing CSVs."""
    pairs: set[tuple[str, str]] = set()
    for path in (TSI, WORD, CUSTOM):
        if not path.exists():
            continue
        for line in path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split(",", 2)
            if len(parts) != 3:
                continue
            word, _freq, bopo = parts
            pairs.add((word, normalize_bopo(bopo)))
    return pairs


def normalize_bopo(bopo: str) -> str:
    """Replace full-width spaces with half-width, collapse whitespace, and
    move neutral-tone marker (˙) from prefix position (moedict convention)
    to suffix position (chewing convention)."""
    bopo = bopo.replace("　", " ")
    syllables = bopo.split()
    fixed = []
    for s in syllables:
        if s.startswith("˙") and len(s) > 1:
            s = s[1:] + "˙"
        fixed.append(s)
    return " ".join(fixed)


def load_moedict() -> list[dict]:
    """Download (if needed) and load the moedict JSON."""
    CACHE_DIR.mkdir(exist_ok=True)
    if not CACHE_JSON.exists():
        if not CACHE_XZ.exists():
            print(f"Downloading {SOURCE_URL}...", file=sys.stderr)
            urllib.request.urlretrieve(SOURCE_URL, CACHE_XZ)
        print(f"Decompressing {CACHE_XZ}...", file=sys.stderr)
        with lzma.open(CACHE_XZ) as f_in:
            CACHE_JSON.write_bytes(f_in.read())
    return json.loads(CACHE_JSON.read_text(encoding="utf-8"))


def main() -> int:
    existing = load_existing_pairs()
    print(f"Loaded {len(existing)} existing (word, bopomofo) pairs", file=sys.stderr)

    entries = load_moedict()
    print(f"Loaded {len(entries)} moedict entries", file=sys.stderr)

    new_pairs: list[tuple[str, str]] = []
    seen_in_output: set[tuple[str, str]] = set()
    skipped_placeholder = 0
    skipped_no_bopo = 0
    skipped_mismatch = 0
    skipped_existing = 0

    for entry in entries:
        title = entry.get("title", "")
        if not title or "{[" in title:
            skipped_placeholder += 1
            continue
        for h in entry.get("heteronyms", []):
            bopo_raw = h.get("bopomofo")
            if not bopo_raw:
                skipped_no_bopo += 1
                continue
            bopo = normalize_bopo(bopo_raw)
            # Sanity: syllable count must match character count.
            if len(bopo.split()) != len(title):
                skipped_mismatch += 1
                continue
            # Skip proverbs/sayings with embedded punctuation — chewing
            # parser rejects non-bopomofo symbols, and they're useless
            # for syllable-by-syllable IM input anyway.
            if any(c in title for c in "，。、；：！？"):
                skipped_mismatch += 1
                continue
            key = (title, bopo)
            if key in existing:
                skipped_existing += 1
                continue
            if key in seen_in_output:
                continue
            seen_in_output.add(key)
            new_pairs.append(key)

    print(
        f"Skipped: placeholder={skipped_placeholder} no_bopo={skipped_no_bopo} "
        f"mismatch={skipped_mismatch} existing={skipped_existing}",
        file=sys.stderr,
    )
    print(f"New entries to emit: {len(new_pairs)}", file=sys.stderr)

    # Stable sort: by word length asc, then word, then bopomofo.
    new_pairs.sort(key=lambda p: (len(p[0]), p[0], p[1]))

    with OUTPUT.open("w", encoding="utf-8") as out:
        out.write("# QBopomofo: 教育部重編國語辭典 詞條（freq=1 fallback）\n")
        out.write("#\n")
        out.write("# 資料來源：https://github.com/g0v/moedict-data\n")
        out.write("# 原始字典：教育部《重編國語辭典（修訂本）》\n")
        out.write("# 原始授權：CC BY-ND 3.0 TW（依教育部解釋，本檔僅取詞條 headword + 注音，不含釋義）\n")
        out.write("# 萃取方式：tools/extract_moedict.py（與既有 tsi.csv/word.csv/phrases.csv 去重後）\n")
        out.write("#\n")
        out.write(f"# 條目數：{len(new_pairs)}\n")
        out.write("\n")
        for word, bopo in new_pairs:
            out.write(f"{word},1,{bopo}\n")

    print(f"Wrote {OUTPUT} ({len(new_pairs)} entries)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
