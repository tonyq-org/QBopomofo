QBopomofo Q注音輸入法 — Windows 安裝說明
==========================================

安裝：
  1. 解壓縮整個 zip 到任意資料夾
  2. 在資料夾內開啟 PowerShell，執行：
       powershell -ExecutionPolicy Bypass -File install.ps1
     （會跳出系統管理員權限確認，請按「是」）
  3. 第一次安裝需手動加入鍵盤：
     設定 → 時間與語言 → 語言與地區 → 中文(台灣) → 語言選項
     → 新增鍵盤 → Q注音輸入法
  4. 從開始功能表搜尋「Q注音設定」，可調整候選排序、每頁數量、
     選字鍵與中英文切換方式。

更新：
  直接執行新版的 install.ps1 即可，會自動覆蓋舊版。

解除安裝：
  powershell -ExecutionPolicy Bypass -File uninstall.ps1

注意：
  - 本程式未購買 Windows 程式碼簽章憑證，下載後第一次執行
    可能出現 SmartScreen 警告，點「其他資訊 → 仍要執行」即可。
  - 原始碼：https://github.com/tonyq-org/QBopomofo
