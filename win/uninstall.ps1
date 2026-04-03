# QBopomofo Windows TSF - Uninstall (PowerShell)
# Must be run as Administrator

$ErrorActionPreference = "Continue"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

$Paths = @(
    (Join-Path $ScriptDir "target\release\qbopomofo_tip.dll"),
    (Join-Path $ScriptDir "target\debug\qbopomofo_tip.dll")
)

foreach ($DllPath in $Paths) {
    if (Test-Path $DllPath) {
        Write-Host "[*] Unregistering $DllPath..."
        Start-Process regsvr32 -ArgumentList "/u /s `"$DllPath`"" -Wait -Verb RunAs
    }
}

# Clean up
[System.Environment]::SetEnvironmentVariable("QBOPOMOFO_DEBUG", $null, "User")
Remove-Item -Path "HKCU:\Software\QBopomofo" -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "[*] QBopomofo has been uninstalled."
Write-Host "   You may need to remove it from your keyboard list in Settings."
