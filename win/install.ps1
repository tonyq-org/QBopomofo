# QBopomofo Windows TSF - Build and Install (PowerShell)
# Must be run as Administrator
#
# Usage:
#   .\install.ps1          - Release build + register
#   .\install.ps1 -Debug   - Debug build + register + enable debug logging

param([switch]$Debug)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

if ($Debug) {
    $BuildMode = "debug"
    $CargoFlag = @()
    [System.Environment]::SetEnvironmentVariable("QBOPOMOFO_DEBUG", "1", "User")
    Write-Host "[*] Debug mode enabled. Log: $env:TEMP\qbopomofo.log"
} else {
    $BuildMode = "release"
    $CargoFlag = @("--release")
}

Write-Host "[*] Building QBopomofo ($BuildMode)..."
Push-Location $ScriptDir
try {
    & cargo build @CargoFlag
    if ($LASTEXITCODE -ne 0) { throw "Build failed" }
} finally {
    Pop-Location
}

$DllPath = Join-Path $ScriptDir "target\$BuildMode\qbopomofo_tip.dll"
if (-not (Test-Path $DllPath)) {
    throw "DLL not found at $DllPath"
}

Write-Host "[*] Registering COM server..."
$reg = Start-Process regsvr32 -ArgumentList "/s `"$DllPath`"" -Wait -PassThru -Verb RunAs
if ($reg.ExitCode -ne 0) {
    throw "regsvr32 failed (exit code $($reg.ExitCode)). Run as Administrator."
}

Write-Host "[*] Done! QBopomofo has been installed."
Write-Host ""
Write-Host "   To use it:"
Write-Host "   1. Open Settings > Time & Language > Language & Region"
Write-Host "   2. Click your language > Language options"
Write-Host "   3. Add a keyboard > Q注音輸入法"
Write-Host ""

if ($Debug) {
    Write-Host "   Debug log: $env:TEMP\qbopomofo.log"
    Write-Host "   View with: Get-Content $env:TEMP\qbopomofo.log -Tail 50"
}
