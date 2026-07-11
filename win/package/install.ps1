# QBopomofo Windows TSF — 安裝（發行包版本）
#
# 用法（在解壓縮後的資料夾內）：
#   powershell -ExecutionPolicy Bypass -File install.ps1
#
# 會自動要求系統管理員權限。安裝內容：
#   1. 複製 qbopomofo_tip.dll、設定程式與字典檔到 %ProgramFiles%\QBopomofo
#   2. regsvr32 註冊 TSF 輸入法
#   3. 重啟 ctfmon 載入新版本
#
# 安裝後到「設定 → 時間與語言 → 語言與地區 → 中文(台灣) → 語言選項
# → 新增鍵盤 → Q注音輸入法」加入。

$ErrorActionPreference = 'Stop'
$src = Split-Path -Parent $MyInvocation.MyCommand.Path

# Elevate if not admin
$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host '[*] 需要系統管理員權限，重新啟動中...'
    Start-Process powershell -Verb RunAs -ArgumentList @(
        '-ExecutionPolicy', 'Bypass', '-File', "`"$($MyInvocation.MyCommand.Path)`""
    )
    exit
}

$dest = Join-Path $env:ProgramFiles 'QBopomofo'
$files = @(
    'qbopomofo_tip.dll',
    'qbopomofo_settings.exe',
    'word.dat',
    'tsi.dat',
    'symbols.dat',
    'swkb.dat'
)

foreach ($f in $files) {
    if (-not (Test-Path (Join-Path $src $f))) {
        throw "找不到 $f — 請在解壓縮後的完整資料夾內執行 install.ps1"
    }
}

Write-Host "[*] 安裝到 $dest ..."
New-Item -ItemType Directory -Force -Path $dest | Out-Null

# Upgrade path: files may be locked by ctfmon / running apps. A locked file
# can't be overwritten but CAN be renamed — rename it out of the way, copy
# the new one in, and clean up stale renames that are no longer held.
$stamp = Get-Date -Format 'yyyyMMddHHmmss'
foreach ($f in $files) {
    $dstF = Join-Path $dest $f
    if (Test-Path $dstF) {
        try { Remove-Item $dstF -Force -Confirm:$false -ErrorAction Stop }
        catch { Rename-Item $dstF "$f.old-$stamp" -Force }
    }
    Copy-Item (Join-Path $src $f) $dstF -Force
}
Get-ChildItem (Join-Path $dest '*.old-*') -ErrorAction SilentlyContinue | ForEach-Object {
    try { Remove-Item $_.FullName -Force -Confirm:$false -ErrorAction Stop } catch {}
}

Write-Host '[*] 註冊 TSF 輸入法...'
$dll = Join-Path $dest 'qbopomofo_tip.dll'
$regsvr = Join-Path $env:WINDIR 'System32\regsvr32.exe'
$registration = Start-Process -FilePath $regsvr -ArgumentList "/s `"$dll`"" -Wait -PassThru
if ($registration.ExitCode -ne 0) {
    throw "regsvr32 註冊失敗 (exit $($registration.ExitCode))"
}

Write-Host '[*] 建立「Q注音設定」開始功能表捷徑...'
$settings = Join-Path $dest 'qbopomofo_settings.exe'
$shortcutPath = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Q注音設定.lnk'
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $settings
$shortcut.WorkingDirectory = $dest
$shortcut.Description = 'Q注音輸入法設定'
$shortcut.Save()

Write-Host '[*] 重啟 ctfmon...'
Stop-Process -Name ctfmon -Force -Confirm:$false -ErrorAction SilentlyContinue
Start-Process "$env:WINDIR\System32\ctfmon.exe" -WindowStyle Hidden

Write-Host ''
Write-Host '[*] 安裝完成！'
Write-Host '    可從開始功能表搜尋「Q注音設定」。'
Write-Host '    若是第一次安裝，請到：'
Write-Host '    設定 → 時間與語言 → 語言與地區 → 中文(台灣) → 語言選項'
Write-Host '    → 新增鍵盤 → Q注音輸入法'
Write-Host ''
Read-Host '按 Enter 結束'
