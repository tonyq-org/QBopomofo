@echo off
REM QBopomofo dev reload — rebuild and reload DLL without logout
REM Usage: dev-reload.bat

setlocal
set "SCRIPT_DIR=%~dp0"
set "OUT_DIR=%SCRIPT_DIR%out"
set "DLL_NAME=qbopomofo_tip.dll"
set "DLL_OUT=%OUT_DIR%\%DLL_NAME%"

REM 1. Build
echo [*] Building (debug)...
cd /d "%SCRIPT_DIR%"
cargo build
if errorlevel 1 (
    echo [!] Build failed.
    exit /b 1
)

REM 2. Unregister DLL first (reduces processes holding it)
echo [*] Unregistering DLL...
where gsudo >nul 2>&1
if %errorlevel%==0 (
    gsudo regsvr32 /u /s "%DLL_OUT%" >nul 2>&1
) else (
    regsvr32 /u /s "%DLL_OUT%" >nul 2>&1
)

REM 3. Kill ALL processes that loaded the DLL + ctfmon + explorer
echo [*] Killing processes that loaded %DLL_NAME%...
powershell -NoProfile -Command ^
  "Get-Process | Where-Object { try { $_.Modules.FileName -like '*qbopomofo*' } catch {} } | ForEach-Object { Write-Host ('  Killing ' + $_.ProcessName + ' (' + $_.Id + ')'); Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }"
taskkill /f /im ctfmon.exe >nul 2>&1
taskkill /f /im explorer.exe >nul 2>&1
timeout /t 2 /nobreak >nul

REM 4. Copy new DLL (try direct copy, fallback to rename-old trick)
if not exist "%OUT_DIR%" mkdir "%OUT_DIR%"
copy /y "%SCRIPT_DIR%target\debug\%DLL_NAME%" "%DLL_OUT%" >nul 2>&1
if errorlevel 1 (
    echo [*] Direct copy failed, trying rename trick...
    del /f "%DLL_OUT%.old" >nul 2>&1
    ren "%DLL_OUT%" "%DLL_NAME%.old" >nul 2>&1
    copy /y "%SCRIPT_DIR%target\debug\%DLL_NAME%" "%DLL_OUT%" >nul
    if errorlevel 1 (
        echo [!] Copy still failed. Close ALL apps and retry.
        start explorer.exe
        start ctfmon.exe
        exit /b 1
    )
    del /f "%DLL_OUT%.old" >nul 2>&1
)

REM 5. Re-register
echo [*] Registering DLL...
where gsudo >nul 2>&1
if %errorlevel%==0 (
    gsudo regsvr32 /s "%DLL_OUT%"
) else (
    regsvr32 /s "%DLL_OUT%"
)

REM 6. Restart explorer and ctfmon
echo [*] Restarting explorer.exe and ctfmon.exe...
start explorer.exe
timeout /t 1 /nobreak >nul
start ctfmon.exe

echo [*] Done! Switch to Q注音 and test.
endlocal
