@echo off
REM QBopomofo Windows TSF - Build and Install
REM Must be run as Administrator (regsvr32 requires elevated privileges)
REM
REM Usage:
REM   install.bat          - Release build + register
REM   install.bat --debug  - Debug build + register + enable debug logging

setlocal

set "SCRIPT_DIR=%~dp0"
set "BUILD_MODE=release"
set "CARGO_FLAG=--release"

if "%1"=="--debug" (
    set "BUILD_MODE=debug"
    set "CARGO_FLAG="
    setx QBOPOMOFO_DEBUG 1 >nul 2>&1
    echo [*] Debug mode enabled. Log will be written to %%TEMP%%\qbopomofo.log
)

echo [*] Building QBopomofo (%BUILD_MODE%)...
cd /d "%SCRIPT_DIR%"
cargo build %CARGO_FLAG%
if errorlevel 1 (
    echo [!] Build failed.
    exit /b 1
)

set "SRC_DLL=%SCRIPT_DIR%target\%BUILD_MODE%\qbopomofo_tip.dll"
if not exist "%SRC_DLL%" (
    echo [!] DLL not found at %SRC_DLL%
    exit /b 1
)

REM Copy to fixed output path (so dev-reload can overwrite without re-registering)
set "OUT_DIR=%SCRIPT_DIR%out"
if not exist "%OUT_DIR%" mkdir "%OUT_DIR%"
copy /y "%SRC_DLL%" "%OUT_DIR%\qbopomofo_tip.dll" >nul
set "DLL_PATH=%OUT_DIR%\qbopomofo_tip.dll"

echo [*] Registering COM server...
where gsudo >nul 2>&1
if %errorlevel%==0 (
    gsudo regsvr32 /s "%DLL_PATH%"
) else (
    regsvr32 /s "%DLL_PATH%"
)
if errorlevel 1 (
    echo [!] regsvr32 failed. Install gsudo (winget install gerardog.gsudo) or run as Administrator.
    exit /b 1
)

echo [*] Adding Q注音 to user input method list...
powershell -Command "$list = Get-WinUserLanguageList; $tip = '0404:{A7E3B4C1-9F2D-4E5A-B8C6-1D3F5A7E9B2C}{B8D1E2F3-6A4C-5D7E-9F0A-2B4C6D8E0F1A}'; $lang = $list | Where-Object { $_.LanguageTag -eq 'zh-Hant-TW' }; if ($lang -and $lang.InputMethodTips -notcontains $tip) { $lang.InputMethodTips.Add($tip); Set-WinUserLanguageList $list -Force }"

echo [*] Done! Q注音輸入法 has been installed.
echo    Use Win+Space or language bar to switch to Q注音.
echo.

if "%1"=="--debug" (
    echo    Debug log: %%TEMP%%\qbopomofo.log
    echo    View with: type %%TEMP%%\qbopomofo.log
)

endlocal
