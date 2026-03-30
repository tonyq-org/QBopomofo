# QBopomofo — 架構設計方案

## Context

目標：基於 Chewing（新酷音）的資料和引擎，打造 QBopomofo（Q注音）跨平台注音輸入法，支援 macOS + Windows。

**核心原則：**
- **打字反應速度第一** — 一切架構決策以延遲最低為優先
- 資料引用 Chewing，但疊隔離層做自己的資料處理
- 引擎能用就用，不能用就覆蓋
- 開源，LGPL 相容
- 雙平台如果共用代價太高，就各幹一套

---

## 一、架構決策：為什麼選擇 Fork libchewing

分析了三種策略後的結論：

| 策略 | 打字延遲 | 開發量 | 可控性 |
|------|---------|--------|--------|
| A. 包裝 C API（外部攔截） | 多一層 FFI 開銷 | 低 | 低 — C API 不開放自訂 Dictionary/Engine |
| **B. Fork libchewing（推薦）** | **零額外開銷** | **中** | **高 — 直接改 Rust trait 實作** |
| C. 從零自建引擎 | 看實作品質 | 極高 | 完全 |

**選 B 的理由：**
- libchewing 的 C API **不允許**從外部注入自訂 Dictionary 或 ConversionEngine
- Fork 後可直接實作 `Dictionary` trait 和 `ConversionEngine` trait，零 FFI 開銷
- Chewing 引擎（K-best path）已成熟，改不如用；資料處理才是我們要客製的
- Fork 不等於重寫 — 我們只加一層 DataAdapter，其餘保持上游同步

---

## 二、隔離層設計 — Build-time Pipeline（非 Runtime）

**關鍵決策：資料處理在編譯期完成，不在打字時跑。**

```
┌─────────────────────────────────────────────────┐
│                 Build-time Pipeline               │
│                                                   │
│  Chewing CSV ──→ DataAdapter ──→ 優化後的 Trie    │
│  (word.csv)      (我們的隔離層)    (二進位格式)     │
│  (tsi.csv)       - 重新排序詞頻                    │
│  (alt.csv)       - 過濾/增補詞彙                   │
│  (symbols.dat)   - 自訂破音字規則                  │
│                  - 合併自訂詞庫                     │
└─────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────┐
│                 Runtime（打字時）                  │
│                                                   │
│  按鍵 → libchewing Engine → 候選字                │
│         (直接讀預編譯 Trie，零額外處理)            │
└─────────────────────────────────────────────────┘
```

**為什麼不在 Runtime 做隔離？**
- 每次按鍵都要查字典，Runtime 多一層轉換 = 多幾毫秒延遲
- 詞庫資料是靜態的（使用者詞庫除外），沒必要每次查詢時處理
- Build-time pipeline 可以做更複雜的處理（NLP 重排、統計分析）而不影響打字速度

### DataAdapter 具體職責

```rust
// data-adapter crate（Build-time 工具）
pub struct DataAdapter {
    chewing_data: ChewingDataSource,   // 讀取 Chewing CSV
    custom_rules: Vec<Box<dyn Rule>>,  // 自訂規則鏈
}

trait Rule {
    fn process(&self, entries: &mut Vec<DictEntry>);
}

// 規則範例：
// - FrequencyBoost: 根據自己的語料重新計算詞頻
// - PhraseFilter: 過濾不需要的詞彙
// - CustomPhrase: 注入自訂詞庫
// - ToneOverride: 覆蓋特定破音字的預設讀音
```

**輸出：** libchewing 原生 Trie 二進位格式 → 引擎直接載入，無需任何適配

---

## 三、專案結構（Monorepo）

引擎源自 libchewing 但不再追蹤上游，全部收進 monorepo，零外部依賴。

```
QBopomofo/                            # Monorepo 根目錄
│
├── base/                             # 共用層
│   ├── engine/                       # 引擎核心（源自 libchewing，獨立發展）
│   │   ├── Cargo.toml                # Workspace member
│   │   ├── LICENSE.upstream          # 上游 LGPL-2.1 授權聲明
│   │   ├── src/
│   │   │   ├── conversion/           # 選字演算法（自由改）
│   │   │   ├── dictionary/           # 字典層（自由改）
│   │   │   ├── editor/               # 組字區
│   │   │   ├── zhuyin/               # 注音處理
│   │   │   └── input/                # 按鍵映射
│   │   └── capi/                     # C API（給 mac/ 用）
│   └── config/                       # 共用設定
│       └── default.toml
│
├── data-provider/                    # 資料隔離層
│   ├── Cargo.toml                    # Build-time pipeline（Workspace member）
│   ├── src/
│   │   ├── lib.rs                    # 核心 pipeline
│   │   ├── chewing_reader.rs         # 讀取 Chewing CSV
│   │   ├── rules/                    # 自訂規則模組
│   │   └── trie_writer.rs            # 輸出 Trie 格式
│   ├── chewing-data/                 # 上游 CSV（直接複製，tracked in git）
│   │   ├── ORIGIN.md                 # 資料來源說明
│   │   ├── word.csv
│   │   ├── tsi.csv
│   │   ├── alt.csv
│   │   └── symbols.dat
│   ├── custom-data/                  # 我們的補充詞庫
│   └── output/                       # 產出的 Trie（.gitignore）
│
├── mac/                              # macOS 實作層
│   ├── Package.swift                 # 依賴 ../base/engine（本地路徑）
│   ├── Sources/
│   │   ├── InputController.swift
│   │   ├── CandidateWindow.swift
│   │   └── Preferences.swift
│   └── Resources/dict/               # → data-provider/output/
│
├── win/                              # Windows 實作層
│   ├── Cargo.toml                    # chewing = { path = "../base/engine" }
│   └── tip/src/
│       ├── text_service/
│       ├── ui/
│       └── config.rs
│
├── Cargo.toml                        # Workspace: [base/engine, data-provider, win]
├── NOTICE                            # 上游版權聲明
├── README.md                         # 專案說明 + 致謝上游
├── .gitignore
├── scripts/
│   └── sync-chewing-data.sh          # 手動從上游拉最新 CSV
└── plans/
    └── architecture.md
```

### 依賴關係（全本地，零網路）

```
mac/ ──→ base/engine (Package.swift 本地路徑, C FFI)
    ──→ data-provider/output/ (預編譯 Trie)
    ──→ base/config/ (讀設定檔)

win/ ──→ base/engine (Cargo workspace member, 直接 Rust)
    ──→ data-provider/output/ (預編譯 Trie)
    ──→ base/config/ (讀設定檔)

data-provider ──→ chewing-data/ (本地 CSV)
              ──→ custom-data/ (我們的補充)
              ──→ base/engine (用其 Trie 格式定義)
```

---

## 四、雙平台策略 — 共用什麼，分開什麼

| 元件 | 共用/分開 | 位置 |
|------|----------|------|
| **引擎核心** | 共用 | `base/engine/`（Cargo workspace + C API） |
| **共用設定** | 共用 | `base/config/` |
| **資料 pipeline** | 共用 | `data-provider/` |
| **字詞庫 CSV** | 共用 | `data-provider/chewing-data/` |
| **自訂詞庫** | 共用 | `data-provider/custom-data/` |
| **macOS 平台層** | 獨立 | `mac/`（Swift + InputMethodKit） |
| **Windows 平台層** | 獨立 | `win/`（Rust + TSF） |

**共用比例約 70%（引擎 + 資料），平台特定約 30%（UI + 系統整合）。**

---

## 五、效能分析 — 打字延遲路徑

### macOS 按鍵到候選字的完整路徑

```
按鍵 (硬體中斷)
  → macOS InputMethodKit 分發 (~0.1ms)
    → Swift InputController.handle() (~0.01ms)
      → C FFI 呼叫 chewing_handle_Default() (~0.001ms FFI overhead)
        → Rust: 注音組合 (~0.01ms)
        → Rust: Trie 查詢 (~0.05ms, 記憶體映射)
        → Rust: K-best path 選字 (~0.1ms)
      → C FFI 取回候選字 (~0.001ms)
    → Swift: 更新候選字 UI (~0.5ms)
  → 畫面更新 (~1ms)

預估總延遲: < 2ms（使用者感知閾值 ~50ms）
```

### 為什麼這個架構夠快

1. **Trie 是預編譯的** — 不需要 Runtime 解析 CSV
2. **libchewing 用 Rust** — 無 GC 停頓，記憶體佈局緊湊
3. **C FFI 呼叫開銷極低** — ~1μs per call
4. **DataAdapter 在 build-time 跑** — 打字時零額外處理
5. **字典用 mmap** — 不佔程序記憶體，OS 管理快取

---

## 六、「能用就用，不能用就覆蓋」的具體策略

引擎已收進 monorepo，不再追蹤上游，可以自由修改。

### 初期直接用的部分
- `base/engine/src/conversion/chewing.rs` — K-best path 選字演算法 ✅
- `base/engine/src/conversion/fuzzy.rs` — 模糊音 ✅
- `base/engine/src/dictionary/trie.rs` — Trie 資料結構 ✅
- `base/engine/src/dictionary/layered.rs` — 分層字典 ✅
- `base/engine/src/dictionary/sqlite.rs` — 使用者詞庫 ✅
- `base/engine/src/zhuyin/` — 注音符號處理 ✅
- `base/engine/src/input/` — 按鍵映射 ✅
- `base/engine/src/editor/zhuyin_layout/` — 鍵盤佈局 ✅
- `base/engine/capi/` — C API 橋接 ✅

### 未來可能改寫的部分
- `estimate.rs` — 換詞頻估算邏輯
- `conversion/` — 加 AI/ML 選字
- `dictionary/loader.rs` — 改字典載入邏輯

### 修改方式
已是我們自己的程式碼，直接改即可。不需要再顧慮上游同步。

---

## 七、執行步驟

### Phase 1: 基礎建設
1. 將 libchewing 原始碼複製進 `base/engine/`
2. 將 libchewing-data CSV 複製進 `data-provider/chewing-data/`
3. 設定 Cargo workspace
4. 建立 `data-provider` crate，實作 CSV 讀取 → Trie 輸出
5. 驗證：引擎能正確載入 data-provider 產出的 Trie

### Phase 2: macOS 輸入法
1. 建立 Xcode 專案（Input Method Bundle）
2. 整合 `base/engine` 作為本地 Swift Package
3. 實作 `IMKInputController` — 按鍵處理 + 基本組字
4. 實作候選字視窗
5. 驗證：能在 macOS 中正常打字

### Phase 3: Windows 輸入法
1. 建立 `win/` Rust 專案，參考 windows-chewing-tsf 的 TSF 架構
2. 整合 `base/engine` 作為 Cargo workspace 依賴
3. 實作 TSF TextService — COM 介面 + 按鍵處理
4. 實作候選字 UI
5. 驗證：能在 Windows 中正常打字

### Phase 4: 客製化（持續）
1. 在 data-provider 中加入自訂規則
2. 補充自訂詞庫
3. 調整 UI/UX
4. 改寫引擎中需要客製的部分

---

## 八、Chewing 可引用資源總覽

| 資源 | 規模 | 引用方式 | Runtime 影響 |
|------|------|---------|-------------|
| word.csv（單字注音表）| 26K 行 | Build-time → Trie | 零 |
| tsi.csv（詞頻庫）| 160K 行 | Build-time → Trie | 零 |
| alt.csv（變音詞）| 25 行 | Build-time → Trie | 零 |
| symbols.dat（符號表）| ~50 類 | 直接載入 | 極低 |
| swkb.dat（符號鍵盤）| ~30 行 | 直接載入 | 極低 |
| ChewingEngine（選字）| — | base/engine 直接用 | 零（原生 Rust） |
| Trie 字典結構 | — | base/engine 直接用 | 零（原生 Rust） |
| 15+ 鍵盤佈局 | — | base/engine 直接用 | 零 |
| 使用者學習機制 | — | base/engine 直接用 | 極低（SQLite） |
| C API + Swift Package | — | base/engine 直接用 | ~1μs per call |

---

## 九、參考專案

| 專案 | 參考價值 |
|------|---------|
| [windows-chewing-tsf](https://github.com/chewing/windows-chewing-tsf) | Windows TSF 整合範本，Rust 實作，GPL-3.0 |
| [McBopomofo](https://github.com/openvanilla/McBopomofo) | macOS 原生注音 IME，Swift 實作，可參考 IMKit 整合 |
| [Fcitx5 macOS](https://github.com/fcitx-contrib/fcitx5-macos) | 另一個 macOS + libchewing 整合方式 |
