# QBopomofo Windows TSF - Build and Install (PowerShell)
#
# Usage:
#   .\install.ps1          - Release build + install
#   .\install.ps1 -Debug   - Debug build + install + enable debug logging
#
# This script builds from source and stages a complete package; the actual
# deployment (copy to %ProgramFiles%\QBopomofo, regsvr32, restart ctfmon,
# Start Menu shortcut) is done by package\install.ps1, which self-elevates.
#
# Do NOT go back to registering win\target\$mode\qbopomofo_tip.dll directly.
# The TIP locates dictionaries via its own DLL directory (see com::dll_dir and
# ActivateEx), and target\ never contains the .dat files. Registering it there
# makes AssetLoader find nothing and the engine silently falls back to the
# builtin mini.dat, which wrecks candidate quality.

param([switch]$Debug)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir

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

$Dicts = @("tsi.dat", "word.dat", "symbols.dat", "swkb.dat")
$DataDir = Join-Path $RepoRoot "data-provider\output"
foreach ($d in $Dicts) {
    if (-not (Test-Path (Join-Path $DataDir $d))) {
        throw "找不到 $d — 請先執行 data-provider/build.sh 產生字典檔"
    }
}

$Stage = Join-Path $ScriptDir "out\install-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
New-Item -ItemType Directory -Force -Path $Stage | Out-Null

foreach ($f in @("qbopomofo_tip.dll", "qbopomofo_settings.exe")) {
    $src = Join-Path $ScriptDir "target\$BuildMode\$f"
    if (-not (Test-Path $src)) { throw "Build output not found at $src" }
    Copy-Item $src $Stage -Force
}
foreach ($d in $Dicts) { Copy-Item (Join-Path $DataDir $d) $Stage -Force }
Copy-Item (Join-Path $ScriptDir "package\install.ps1") $Stage -Force
Copy-Item (Join-Path $ScriptDir "package\uninstall.ps1") $Stage -Force

Write-Host "[*] Staged package: $Stage"
Write-Host "[*] Handing off to the packaged installer (will request admin)..."
& (Join-Path $Stage "install.ps1")
