# QBopomofo Windows TSF — 解除安裝（發行包版本）
#
# 用法：
#   powershell -ExecutionPolicy Bypass -File uninstall.ps1

$ErrorActionPreference = 'Continue'

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
$dll = Join-Path $dest 'qbopomofo_tip.dll'

if (Test-Path $dll) {
    Write-Host '[*] 解除註冊 TSF 輸入法...'
    & regsvr32 /u /s $dll
}

Write-Host '[*] 重啟 ctfmon 釋放 DLL...'
Stop-Process -Name ctfmon -Force -Confirm:$false -ErrorAction SilentlyContinue
Start-Process "$env:WINDIR\System32\ctfmon.exe"
Start-Sleep -Seconds 1

if (Test-Path $dest) {
    Write-Host "[*] 移除 $dest ..."
    try {
        Remove-Item $dest -Recurse -Force -Confirm:$false -ErrorAction Stop
    } catch {
        Write-Host '[!] 部分檔案仍被使用中，重開機後可手動刪除該資料夾。'
    }
}

# Clean up per-user settings
[System.Environment]::SetEnvironmentVariable('QBOPOMOFO_DEBUG', $null, 'User')
Remove-Item -Path 'HKCU:\Software\QBopomofo' -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ''
Write-Host '[*] 已解除安裝。'
Write-Host '    請到「設定 → 鍵盤」把 Q注音輸入法 從清單移除。'
Write-Host ''
Read-Host '按 Enter 結束'
