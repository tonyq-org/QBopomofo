# QBopomofo — Q注音輸入法

跨平台智慧注音輸入法，支援 macOS 與 Windows。

本專案的引擎核心與字詞庫資料源自 [Chewing（酷音）](https://chewing.im/) 開源專案，由 [libchewing Core Team](https://codeberg.org/chewing/libchewing) 及社群貢獻者多年維護。我們在此基礎上進行獨立發展。

## 上游專案

| 專案 | 說明 | 授權 |
|------|------|------|
| [libchewing](https://codeberg.org/chewing/libchewing) | 智慧注音輸入法引擎 | LGPL-2.1 |
| [libchewing-data](https://codeberg.org/chewing/libchewing-data) | 字詞庫與詞頻資料 | LGPL-2.1 |

## 專案結構

```
QBopomofo/
├── base/engine/         # 引擎核心（源自 libchewing，獨立發展）
├── base/config/         # 共用設定
├── data-provider/       # 資料隔離層（build-time 處理 pipeline）
├── mac/                 # macOS 實作（Swift + InputMethodKit）
├── win/                 # Windows 實作（Rust + TSF）
└── plans/               # 架構文件
```

## 與上游的關係

- `base/engine/` 的初始程式碼來自 libchewing，之後獨立發展，不再追蹤上游更新
- `data-provider/chewing-data/` 的 CSV 資料來自 libchewing-data，可視需要手動同步
- 詳見 [NOTICE](./NOTICE) 了解完整版權聲明

## 授權

本專案以 LGPL-2.1-or-later 授權釋出。詳見 [LICENSE](./LICENSE)。
