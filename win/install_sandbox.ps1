# QBopomofo Sandbox Install
# Build the DLL (static CRT), copy to out/, and launch Windows Sandbox.
#
# Usage:
#   .\install_sandbox.ps1            # release build
#   .\install_sandbox.ps1 -debug     # debug build

param([switch]$debug)

$ErrorActionPreference = 'Stop'
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$outDir = Join-Path $scriptDir 'out'

# Static CRT so Sandbox doesn't need VC++ Redistributable
$env:RUSTFLAGS = '-C target-feature=+crt-static'
# Use separate target dir to avoid conflicts with Sandbox mapping
$env:CARGO_TARGET_DIR = Join-Path $scriptDir 'target3'

# 1. Build
if ($debug) {
    Write-Host '[*] Building (debug, static CRT)...'
    cargo build --manifest-path "$scriptDir\Cargo.toml"
    $dllSrc = Join-Path $env:CARGO_TARGET_DIR 'debug\qbopomofo_tip.dll'
} else {
    Write-Host '[*] Building (release, static CRT)...'
    cargo build --manifest-path "$scriptDir\Cargo.toml" --release
    $dllSrc = Join-Path $env:CARGO_TARGET_DIR 'release\qbopomofo_tip.dll'
}

if ($LASTEXITCODE -ne 0) {
    Write-Host '[!] Build failed.'
    exit 1
}

# 2. Copy DLL to out/
if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Path $outDir | Out-Null }
Copy-Item $dllSrc (Join-Path $outDir 'qbopomofo_tip.dll') -Force
Write-Host "[*] DLL copied to $outDir"

# 3. Launch Sandbox
$wsbFile = Join-Path $scriptDir 'test-sandbox.wsb'
Write-Host '[*] Launching Windows Sandbox...'
Start-Process $wsbFile
Write-Host '[*] Sandbox started. It will auto-register Q注音 and open Notepad.'
