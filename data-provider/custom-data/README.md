# 自訂詞庫 (custom-data)

此目錄存放 QBopomofo 的自訂詞頻調整和新增詞彙。

## 檔案

| 檔案 | 用途 |
|------|------|
| `phrases.csv` | 自訂詞庫：新增詞彙或覆蓋上游詞頻 |
| `moedict-phrases.csv` | 教育部《重編國語辭典（修訂本）》詞條（freq=1 fallback） |

## 資料來源

`moedict-phrases.csv` 由 [`tools/extract_moedict.py`](../tools/extract_moedict.py) 從
[g0v/moedict-data](https://github.com/g0v/moedict-data) 萃取，原始字典為
教育部《重編國語辭典（修訂本）》（CC BY-ND 3.0 TW）。本檔僅含詞條 headword + 注音，
不含釋義；依教育部解釋此種利用方式不受 ND 限制。詳見根目錄 `NOTICE` 檔。

## 使用方式

使用 `/tune` skill 來新增或調整詞彙，它會自動：
1. 分析你的需求是否合理
2. 檢查與現有詞庫是否衝突
3. 寫入 phrases.csv
4. 重建 trie 並建議測試

## 格式

```csv
詞,詞頻,注音1 注音2 ...
再試試,5000,ㄗㄞˋ ㄕˋ ㄕˋ
```

## 詞頻參考

- 50000+ : 超高頻（的、是、我、不）
- 5000-50000 : 高頻（已經、可以、因為）
- 500-5000 : 中頻（嘗試、調整）
- 0-500 : 低頻
