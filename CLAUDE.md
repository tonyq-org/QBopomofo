# CLAUDE.md — 專案開發指引

## 第一原則：效能

**打字反應速度是本專案的最高優先級。沒有例外。**

任何架構決策、程式碼變更、功能新增，都必須以「不增加打字延遲」為前提。
如果一個設計更優雅但更慢，我們選更快的那個。
如果一個功能很酷但會拖慢按鍵響應，我們不做。

### 效能紅線

- 按鍵到候選字的端到端延遲必須 < 5ms（目標 < 2ms）
- **禁止在按鍵處理的 hot path 中做任何 I/O、網路請求、或動態資料轉換**
- 字典查詢必須走預編譯的 Trie（mmap），不得在 Runtime 解析 CSV 或其他文字格式
- 資料處理（詞頻調整、詞庫合併等）只能在 build-time 做，絕不在打字時做
- 不得引入有 GC 的語言或 Runtime 到按鍵處理路徑中
- 候選字 UI 更新不得阻塞按鍵處理

### 效能審查清單

每次修改程式碼前，問自己：
1. 這段程式碼會在使用者按鍵時執行嗎？
2. 如果會，它增加了多少延遲？
3. 有沒有辦法把這個工作移到 build-time 或背景執行緒？

**如果答案是「會增加可感知的延遲」，就不要合併。**

---

## 專案結構

```
chewing/
├── base/engine/         # 引擎核心（源自 libchewing，獨立發展）
├── base/config/         # 共用設定
├── data-provider/       # 資料隔離層（build-time pipeline）
├── mac/                 # macOS 實作（Swift + InputMethodKit）
├── win/                 # Windows 實作（Rust + TSF）
└── plans/               # 架構文件
```

## 上游關係

- 引擎源自 [libchewing](https://codeberg.org/chewing/libchewing)（LGPL-2.1），已獨立發展，不追蹤上游
- 字詞庫源自 [libchewing-data](https://codeberg.org/chewing/libchewing-data)（LGPL-2.1）
- 詳見 NOTICE 檔

## 架構原則

- **build-time > runtime** — 能在編譯期做的事不要留到打字時做
- **本地 > 網路** — 全 monorepo，零外部依賴，build 不需要網路
- **能用就用，不能用就改** — 引擎已是我們的程式碼，直接改不用客氣
- **平台層各寫一套** — mac/ 和 win/ 不強求共用 UI 程式碼，各自用原生方案

## 開發語言

- 引擎：Rust
- macOS 平台層：Swift（透過 C API 呼叫引擎）
- Windows 平台層：Rust（直接引用引擎 crate）
- 資料處理工具：Rust
