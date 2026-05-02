# run-dev.ps1 — one-shot dev loop for QBopomofo Windows TIP.
#
# Builds the workspace and launches dev_host.exe — a Win32 window that
# drives `Controller` + `CandidateWindow` directly (no TSF, no admin,
# no regsvr32). Iterate on controller logic and candidate UI without
# touching the system input stack.
#
# Env:
#   $env:CHEWING_PATH  Path to dictionary dir (default: data-provider/output/)

param(
    [switch]$Release
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $here

if (-not $env:CHEWING_PATH) {
    $env:CHEWING_PATH = (Resolve-Path "$here\..\data-provider\output").Path
    Write-Host "[info] CHEWING_PATH -> $env:CHEWING_PATH"
}

$profileArg = if ($Release) { '--release' } else { $null }
$outDir = if ($Release) { 'release' } else { 'debug' }

Write-Host "[info] cargo build"
if ($profileArg) {
    cargo build $profileArg --bins --lib
} else {
    cargo build --bins --lib
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$exe = Join-Path $here "target\$outDir\dev_host.exe"
if (-not (Test-Path $exe)) {
    Write-Error "dev_host.exe not found at $exe"
    exit 1
}

Write-Host "[info] launching $exe"
& $exe
