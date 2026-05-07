#!/usr/bin/env python3
"""
Generate Taiwan political figure name entries for the custom dictionary.

This script is intentionally manual/offline from the normal build path. It may
fetch open government data when run, but data-provider/build.sh never does.
"""
from __future__ import annotations

import csv
import json
import re
import ssl
import sys
import urllib.error
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
LY_TERM11_URL = (
    "https://data.ly.gov.tw/odw/ID16Action.action?"
    "name=&sex=&party=&partyGroup=&areaName=&term=11&fileType=json"
)
OUTPUT = ROOT / "custom-data" / "tw-politicians.csv"

SOURCE_CSVS = [
    ROOT / "chewing-data" / "tsi.csv",
    ROOT / "chewing-data" / "word.csv",
    ROOT / "custom-data" / "phrases.csv",
    ROOT / "custom-data" / "moedict-phrases.csv",
]

# High-profile officeholders, party figures, and commonly typed political names
# not guaranteed to appear in the Legislative Yuan dataset.
SEED_NAMES = """
賴清德 蕭美琴 卓榮泰 鄭麗君 韓國瑜 江啟臣 蔡其昌 朱立倫 黃國昌 柯文哲 蔡英文
侯友宜 盧秀燕 蔣萬安 陳其邁 張善政 黃偉哲 謝國樑 高虹安 邱臣遠 林右昌
林佳龍 陳建仁 蘇貞昌 游錫堃 賴士葆 王金平 馬英九 陳水扁 李登輝 宋楚瑜
郝龍斌 謝長廷 蘇巧慧 林岱樺 林智堅 鄭文燦 潘孟安 吳釗燮 劉世芳 顧立雄
沈伯洋 苗博雅 王義川 王世堅 黃捷 徐巧芯 吳思瑤 王鴻薇 羅智強 鍾小平
于美人 黃珊珊 陳時中 陳柏惟 顏寬恒 謝龍介 陳亭妃 蘇煥智 傅崐萁 柯建銘
王定宇 林俊憲 高嘉瑜 羅文嘉 陳菊 張麗善 翁章梁 周春米 徐榛蔚 鍾東錦
許淑華 王惠美 楊文科 林姿妙 饒慶鈴 陳光復 陳福海 王忠銘
""".split()

# Overrides are for common name readings where the highest-frequency dictionary
# reading is not necessarily the personal-name reading.
READING_OVERRIDES = {
    "沈": "ㄕㄣˇ",
    "單": "ㄕㄢˋ",
    "曾": "ㄗㄥ",
    "游": "ㄧㄡˊ",
    "陸": "ㄌㄨˋ",
    "長": "ㄔㄤˊ",
    "發": "ㄈㄚ",
    "顥": "ㄏㄠˋ",
    "崐": "ㄎㄨㄣ",
    "萁": "ㄑㄧˊ",
    "葆": "ㄅㄠˇ",
    "堃": "ㄎㄨㄣ",
    "釗": "ㄓㄠ",
    "燮": "ㄒㄧㄝˋ",
    "顧": "ㄍㄨˋ",
    "鴻": "ㄏㄨㄥˊ",
    "薇": "ㄨㄟ",
    "惟": "ㄨㄟˊ",
    "恒": "ㄏㄥˊ",
    "榛": "ㄓㄣ",
    "蔚": "ㄨㄟˋ",
    "樑": "ㄌㄧㄤˊ",
    "邁": "ㄇㄞˋ",
}


def fetch_json(url: str) -> dict:
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": "QBopomofo dictionary generator/0.1",
            "Accept": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except urllib.error.URLError:
        # data.ly.gov.tw has historically had certificate-chain quirks on some
        # macOS/Python combinations. Keep this script usable while still keeping
        # network access out of the normal build path.
        ctx = ssl._create_unverified_context()
        with urllib.request.urlopen(req, timeout=30, context=ctx) as resp:
            return json.loads(resp.read().decode("utf-8"))


def normalize_name(raw: str) -> str | None:
    name = "".join(re.findall(r"[\u4e00-\u9fff]+", raw))
    if 2 <= len(name) <= 4:
        return name
    return None


def fetch_legislator_names() -> set[str]:
    data = fetch_json(LY_TERM11_URL)
    names = set()
    for row in data.get("dataList", []):
        if row.get("leaveFlag") == "是":
            continue
        name = normalize_name(row.get("name", ""))
        if name:
            names.add(name)
    return names


def load_single_char_readings() -> dict[str, str]:
    best: dict[tuple[str, str], int] = {}
    for path in SOURCE_CSVS:
        if not path.exists():
            continue
        with path.open(encoding="utf-8") as f:
            for row in csv.reader(f):
                if not row or row[0].startswith("#") or len(row) < 3:
                    continue
                word = row[0].strip()
                bopomofo = row[2].split("#", 1)[0].strip().replace("　", " ")
                syllables = bopomofo.split()
                if len(word) != 1 or len(syllables) != 1:
                    continue
                try:
                    freq = int(row[1].strip())
                except ValueError:
                    freq = 0
                key = (word, syllables[0])
                best[key] = max(best.get(key, -1), freq)

    by_char: dict[str, tuple[str, int]] = {}
    for (char, bopomofo), freq in best.items():
        if char not in by_char or freq > by_char[char][1]:
            by_char[char] = (bopomofo, freq)

    readings = {char: bopomofo for char, (bopomofo, _freq) in by_char.items()}
    readings.update(READING_OVERRIDES)
    return readings


def name_to_bopomofo(name: str, readings: dict[str, str]) -> str | None:
    syllables = []
    for char in name:
        bopomofo = readings.get(char)
        if not bopomofo:
            return None
        syllables.append(bopomofo)
    return " ".join(syllables)


def main() -> int:
    names = set(SEED_NAMES)
    names.update(fetch_legislator_names())

    readings = load_single_char_readings()
    rows = []
    missing = []
    for name in sorted(names):
        bopomofo = name_to_bopomofo(name, readings)
        if bopomofo:
            rows.append((name, bopomofo))
        else:
            missing.append(name)

    if missing:
        print("Missing readings for: " + ", ".join(missing), file=sys.stderr)

    with OUTPUT.open("w", encoding="utf-8", newline="") as f:
        f.write("# QBopomofo: Taiwan political figure names\n")
        f.write("#\n")
        f.write("# Sources:\n")
        f.write("# - Legislative Yuan Open Data Platform, dataset 16: 歷屆委員資料, term=11\n")
        f.write("#   https://data.ly.gov.tw/getds.action?id=16\n")
        f.write("#   License: Government Open Data License, version 1.0\n")
        f.write("# - QBopomofo manually curated high-profile Taiwan political figure seed list\n")
        f.write("#\n")
        f.write("# Generated by data-provider/tools/generate_tw_politicians.py\n")
        f.write(f"# Entries: {len(rows)}\n")
        f.write("\n")
        writer = csv.writer(f, lineterminator="\n")
        for name, bopomofo in rows:
            writer.writerow([name, 1000, bopomofo])

    print(f"Wrote {OUTPUT} ({len(rows)} entries)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
