@echo off
REM QBopomofo Windows TSF - Uninstall
REM Must be run as Administrator

setlocal

set "SCRIPT_DIR=%~dp0"

REM Try both release and debug paths
set "DLL_RELEASE=%SCRIPT_DIR%target\release\qbopomofo_tip.dll"
set "DLL_DEBUG=%SCRIPT_DIR%target\debug\qbopomofo_tip.dll"

where gsudo >nul 2>&1
if %errorlevel%==0 ( set "SUDO=gsudo" ) else ( set "SUDO=" )

if exist "%DLL_RELEASE%" (
    echo [*] Unregistering release DLL...
    %SUDO% regsvr32 /u /s "%DLL_RELEASE%"
)

if exist "%DLL_DEBUG%" (
    echo [*] Unregistering debug DLL...
    %SUDO% regsvr32 /u /s "%DLL_DEBUG%"
)

REM Clean up debug env var
setx QBOPOMOFO_DEBUG "" >nul 2>&1

REM Clean up registry preferences
reg delete "HKCU\Software\QBopomofo" /f >nul 2>&1

echo [*] QBopomofo has been uninstalled.
echo    You may need to remove it from your keyboard list in Settings.

endlocal
