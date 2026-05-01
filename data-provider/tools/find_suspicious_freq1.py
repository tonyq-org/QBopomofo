#!/usr/bin/env python3
"""
Find single-char entries in tsi.csv where freq=1 but the same (char, bopomofo)
appears in high-freq compound phrases — strong signal that the upstream
single-char freq is mis-set and needs custom tuning.

Output is sorted by highest compound-phrase freq containing the suspect
character; the top of the list is the most egregious cases.
"""
from __future__ import annotations

import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TSI = ROOT / "chewing-data" / "tsi.csv"
CUSTOM = ROOT / "custom-data" / "phrases.csv"

MIN_COMPOUND_FREQ = 100   # only flag if some compound containing the char has this much freq
MIN_HOMOPHONE_FREQ = 0    # report regardless of homophone single-char freq


def parse_csv(path: Path):
    """Yield (word, freq, bopomofo) tuples, skipping comments/blank lines."""
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split(",", 2)
        if len(parts) != 3:
            continue
        word, freq, bopo = parts
        try:
            freq = int(freq)
        except ValueError:
            continue
        yield word, freq, bopo


def main() -> int:
    # Already-tuned (char, bopo) pairs from custom phrases — skip these.
    tuned: set[tuple[str, str]] = set()
    if CUSTOM.exists():
        for word, _freq, bopo in parse_csv(CUSTOM):
            syls = bopo.split()
            if len(word) == len(syls):
                for ch, s in zip(word, syls):
                    tuned.add((ch, s))

    # Pass 1 — collect all single-char freqs per (char, bopo).
    singles: dict[tuple[str, str], int] = {}
    # And per (char, bopo) the best compound phrase containing that char-syllable.
    best_phrase: dict[tuple[str, str], tuple[int, str]] = {}
    # And homophones: for each bopo, list of (char, freq) for single chars.
    homophones: dict[str, list[tuple[str, int]]] = defaultdict(list)

    for word, freq, bopo in parse_csv(TSI):
        syls = bopo.split()
        if len(word) != len(syls):
            continue
        if len(word) == 1:
            singles[(word, bopo)] = max(singles.get((word, bopo), 0), freq)
            homophones[bopo].append((word, freq))
            continue
        # Multi-char phrase — credit each (char, syllable) with this phrase's freq.
        for ch, s in zip(word, syls):
            cur = best_phrase.get((ch, s))
            if cur is None or freq > cur[0]:
                best_phrase[(ch, s)] = (freq, word)

    # Build report.
    suspects = []
    for (ch, bopo), freq in singles.items():
        if freq != 1:
            continue
        if (ch, bopo) in tuned:
            continue
        bp = best_phrase.get((ch, bopo))
        if not bp or bp[0] < MIN_COMPOUND_FREQ:
            continue
        compound_freq, compound = bp
        # Top homophone for this bopo (the single char that currently wins).
        homo = sorted(homophones[bopo], key=lambda x: -x[1])
        top_char, top_freq = homo[0] if homo else ("", 0)
        suspects.append({
            "ch": ch,
            "bopo": bopo,
            "compound": compound,
            "compound_freq": compound_freq,
            "top_homophone": top_char,
            "top_homophone_freq": top_freq,
        })

    suspects.sort(key=lambda s: -s["compound_freq"])

    # Heuristic: custom_freq = compound_freq.
    # Setting the single-char freq equal to the strongest compound usage lets
    # candidates rank by genuine frequency: dominant chars win, secondary chars
    # land at rank 2/3. Avoids over-shooting past legitimate homophones.
    for s in suspects:
        s["proposed"] = s["compound_freq"]

    print(f"# Suspect single-char entries (freq=1 with strong compound usage)")
    print(f"# Found {len(suspects)} cases. Threshold: compound freq >= {MIN_COMPOUND_FREQ}")
    print(f"# Proposed = compound_freq (let real-world usage decide ranking)")
    print()
    print(f"{'CH':<3}{'BOPO':<24}{'COMPOUND':<14}{'CF':>8}{'TOP_HOMO':>10}{'HF':>8}{'PROP':>8}")
    print("-" * 75)
    for s in suspects:
        print(f"{s['ch']:<3}{s['bopo']:<24}{s['compound']:<14}{s['compound_freq']:>8}{s['top_homophone']:>10}{s['top_homophone_freq']:>8}{s['proposed']:>8}")

    print()
    print(f"# CSV-paste-ready (all {len(suspects)}):")
    for s in suspects:
        print(f"{s['ch']},{s['proposed']},{s['bopo']}  # 上游 freq=1; 補強（{s['compound']} {s['compound_freq']}）")

    return 0


if __name__ == "__main__":
    sys.exit(main())
