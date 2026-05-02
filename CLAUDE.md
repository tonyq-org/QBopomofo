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

## 開源授權合規（嚴格執行）

**引入任何第三方程式碼或函式庫前，必須確認以下事項：**

1. **授權相容性** — 本專案為 LGPL-2.1-or-later，引入的依賴必須與 LGPL-2.1 相容
   - 相容：MIT, BSD-2, BSD-3, Apache-2.0, Zlib, ISC, Unlicense, LGPL-2.1+, MPL-2.0
   - 需注意：GPL-2.0/3.0（會感染整個專案為 GPL）
   - 不相容：AGPL, SSPL, 任何禁止商用的授權, proprietary
2. **標註出處** — 在 NOTICE 檔中記錄每個引用的開源專案：名稱、版權、授權、來源 URL
3. **保留原始授權** — 不得移除任何第三方程式碼中的版權聲明和授權文字
4. **Cargo 依賴審查** — 新增 Rust crate 依賴前，用 `cargo license` 檢查其授權鏈
5. **Swift 依賴審查** — 新增 Swift Package 前，確認其 LICENSE 檔案

### 目前使用的開源專案

| 專案 | 授權 | 用途 | 位置 |
|------|------|------|------|
| [libchewing](https://codeberg.org/chewing/libchewing) | LGPL-2.1 | 引擎核心（已 fork） | `base/engine/` |
| [libchewing-data](https://codeberg.org/chewing/libchewing-data) | LGPL-2.1 | 字詞庫 CSV 資料 | `data-provider/chewing-data/` |

**新增依賴時務必更新此表。**

## macOS 輸入法部署

### Build 與安裝腳本

| 腳本 | 用途 |
|------|------|
| `mac/build-app.sh` | 編譯 Rust 引擎 + Swift，組裝 `.app` bundle |
| `mac/install.sh` | build + 安裝到 `~/Library/Input Methods/` + 註冊 |
| `mac/install.sh --debug` | 同上，啟用 debug log 寫入 `/tmp/qbopomofo.log` |
| `mac/TestApp/run.sh` | 編譯並啟動 TestApp（開發用） |

### 部署踩坑紀錄

1. **Rust 與 Swift 必須一起重建** — `swift package clean && swift build` 不會重建 Rust `libchewing_capi.a`。必須先跑 `cd base/engine/capi && cargo build --release`，再 build Swift。所有 build 腳本已包含此步驟。
2. **`InputMethodServerControllerClass` 必須匹配 ObjC 名稱** — Swift class 有 `@objc(QBopomofoInputController)`，所以 Info.plist 裡要寫 `QBopomofoInputController`（不帶 module prefix）。寫 `QBopomofo.QBopomofoInputController` 會導致 IMKServer 找不到 controller。
3. **`LSBackgroundOnly` 不能設** — 會阻擋 key event 送達，只需要 `LSUIElement = true`。
4. **macOS 不一定自動啟動輸入法程序** — `install.sh` 會在安裝後手動啟動程序。
5. **更新後需要殺掉舊 process** — `install.sh` 會自動 `pkill` 舊的 QBopomofo。
6. **`NSApplication.shared` 必須在 `IMKServer` 之前初始化** — 否則 IMKServer 可能無法正確接收事件。
7. **確認版本有更新** — build 腳本會產生 `BuildInfo.swift`（含 build timestamp），啟動時印出版本。debug 模式寫到 `/tmp/qbopomofo.log`。
8. **首次安裝需手動加入** — 系統設定 → 鍵盤 → 輸入方式 → + → Q注音。之後更新不需要重複此步驟。

### Debug 模式

```bash
# 啟動 debug 模式
cd mac && ./install.sh --debug

# 查看 log
tail -f /tmp/qbopomofo.log
```

環境變數 `QBOPOMOFO_DEBUG=1` 啟用 debug log。正式使用時不設此變數，零 I/O 開銷。

## Windows 輸入法部署與開發

### 疊代迴圈（無須 Windows Sandbox）

正式 TIP DLL (`qbopomofo_tip.dll`) 被 `regsvr32` 註冊後，會被所有用 TSF 的應用 load 住 → 改 code 後必須全部殺掉才能覆蓋 DLL，這也是舊流程只能靠 sandbox 的原因。

**新流程**：

| 工具 | 用途 |
|------|------|
| `cargo run --bin dev_harness` | 無 COM、無視窗的 CLI；stdin 打 `TYPE 5j/` / `KEY 0x0D` 驗輸入邏輯；設 `CHEWING_PATH` |
| `win/run-dev.ps1` | 一鍵 build + 跑 `dev_host.exe`。**不是真 TSF host**：直接 link `Controller` + `CandidateWindow`，在 message pump 攔 `WM_KEYDOWN` 餵給 controller，commit 走 `EM_REPLACESEL`、preedit 顯示在標題列、候選用 `CandidateWindow`。驗輸入邏輯 + 候選視窗視覺，無 admin 無 regsvr32 |
| `win/install.ps1` | 正式 TSF 註冊到系統（HKLM regsvr32）。需要測 TSF edit session / composition lifecycle / 跨 app 行為時用這條 |

### 部署踩坑紀錄

1. **TSF 真 host 需要 admin 才能註冊，所以 dev_host 不走 TSF** — `ITfInputProcessorProfiles::Register` + `ITfCategoryMgr::RegisterCategory` 都寫 HKLM，一般使用者會噴 E_FAIL。即使 CoCreateInstance 能在 HKCU 載到 TIP，`AdviseKeyEventSink` 仍會拒絕非「正在被 TSF 啟動」的 tfClientId → E_INVALIDARG。dev_host 改成直接驅動 `Controller`，把 TSF 那層整個拿掉。真 TSF 整合測試只能用 `install.ps1`。
2. **`install.ps1` 註冊後 DLL 被鎖** — ctfmon + 所有 TSF 應用會 load 住 DLL，改 code 重 build 要先全部殺掉。這條路徑維持正式部署用，日常開發靠 dev_host 繞過。
3. **Rust panic 跨 `extern "system"` 是 UB** — 所有 COM method 都要套 `com_method_*!` macro（`panic_guard.rs`），panic 會寫 `%TEMP%\qbopomofo_crash.log` 後回 `E_FAIL` 而不是拖 host 陪葬。
4. **候選視窗 PaintData magic number** — WndProc 前先驗 `GWLP_USERDATA` 裡的 magic，防 DestroyWindow 後殘留訊息去 deref 野指標。
5. **HiDPI / 多螢幕 / 深色模式** — `candidate_window.rs` 走 `GetDpiForWindow` + `MonitorFromPoint` + `AppsUseLightTheme` 三件套；改視覺時不要寫死 px。

### Debug 模式

```powershell
# 開發迴圈（推薦）：
cd win
./run-dev.ps1                # debug build + launch dev_host
# 在彈出的 RichEdit 視窗直接打字測試；關窗回到 terminal 改 code 再跑
```

`win/src/controller.rs` 是平台無關核心邏輯；`text_service.rs` 只是薄 COM wrapper。單元測試用 `dev_harness` 比對 stdout 事件序列，不需要實機視窗。
