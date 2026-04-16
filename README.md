# QBopomofo — Q注音輸入法

跨平台智慧注音輸入法，支援 macOS 與 Windows。

本專案的引擎核心與字詞庫資料源自 [Chewing（酷音）](https://chewing.im/) 開源專案，由 [libchewing Core Team](https://codeberg.org/chewing/libchewing) 及社群貢獻者多年維護。我們在此基礎上進行獨立發展。

## 上游專案與分歧版本

本專案源自以下上游專案，並於下列版本分歧後獨立發展：

| 專案 | 說明 | 授權 | 分歧點 |
|------|------|------|--------|
| [libchewing](https://codeberg.org/chewing/libchewing) | 智慧注音輸入法引擎 | LGPL-2.1 | [`100a0e0`](https://codeberg.org/chewing/libchewing/commit/100a0e09178532c570cc1680c97bc7541617426a)（2026-03-28） |
| [libchewing-data](https://codeberg.org/chewing/libchewing-data) | 字詞庫與詞頻資料 | LGPL-2.1 | [`dd81960`](https://codeberg.org/chewing/libchewing-data/commit/dd81960c90a75d07c3a80b542d721694cc034665)（2026-03-26） |

> 分歧後不再追蹤上游更新，引擎與資料均獨立維護。

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

## macOS 安裝

需求：

- macOS 13 以上
- Xcode Command Line Tools
- Swift 6.1 以上
- Rust toolchain

### Apple Silicon / M 系列 Mac 從零安裝

如果另一台 Mac 還沒有安裝開發環境，先執行：

```bash
xcode-select --install
```

如需安裝 Homebrew：

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
echo 'eval "$(/opt/homebrew/bin/brew shellenv)"' >> ~/.zprofile
eval "$(/opt/homebrew/bin/brew shellenv)"
```

安裝 Rust：

```bash
brew install rust
```

確認工具可用：

```bash
swift --version
rustc --version
cargo --version
```

### 安裝 Q注音

在目標 Mac 上執行：

```bash
git clone https://github.com/lionello06160/QBopomofo.git
cd QBopomofo/mac
./install.sh
```

安裝完成後，到「系統設定 → 鍵盤 → 輸入方式 → +」加入 `Q注音`。

如果是使用 Homebrew 安裝 Rust（特別是 Apple Silicon / `/opt/homebrew` 環境），目前 repo 已內建處理 Swift build plugin 的 PATH，不需要額外手動設定 `rustc` 路徑。

### 安裝後啟用

1. 打開「系統設定 → 鍵盤 → 輸入方式」
2. 按 `+`
3. 加入 `Q注音`
4. 切換到 `Q注音` 開始使用

如果安裝後沒有立刻出現在輸入方式清單：

- 先完全退出正在使用的 app 再重新打開
- 重新切換一次輸入法
- 必要時登出再登入 macOS

### 重新安裝 / 更新

如果你已經 clone 過 repo，之後更新只要：

```bash
cd QBopomofo
git pull
cd mac
./install.sh
```

### 建立 `.pkg` 安裝程式

如果你要發給其他 Mac 直接點兩下安裝，可以在開發機上產生 `.pkg`：

```bash
cd QBopomofo/mac
./build-pkg.sh
```

產生出的檔案會在：

```bash
mac/.build/QBopomofo-Installer.pkg
```

目前這是未 notarize 的安裝骨架版本，適合內部測試或自用機器。目標 Mac 安裝後，app 會被放到：

```bash
/Library/Input Methods/QBopomofo.app
```

安裝完成後，仍需到「系統設定 → 鍵盤 → 輸入方式 → +」手動加入 `Q注音`。

### 常見問題

`cargo: command not found`

- 代表 Rust 尚未安裝，請先執行 `brew install rust`

`swift: command not found`

- 代表 Xcode Command Line Tools 尚未安裝，請先執行 `xcode-select --install`

安裝成功但無法切到 `Q注音`

- 先到「系統設定 → 鍵盤 → 輸入方式」確認已加入 `Q注音`
- 如果已加入但 app 內無法使用，先完全退出該 app 再重新開啟

## 與上游的關係

- `base/engine/` 的初始程式碼來自 libchewing，之後獨立發展，不再追蹤上游更新
- `data-provider/chewing-data/` 的 CSV 資料來自 libchewing-data，可視需要手動同步
- 詳見 [NOTICE](./NOTICE) 了解完整版權聲明

## 授權

本專案以 LGPL-2.1-or-later 授權釋出。詳見 [LICENSE](./LICENSE)。
