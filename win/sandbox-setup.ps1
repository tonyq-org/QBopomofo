# QBopomofo Sandbox Setup — runs inside Windows Sandbox
# Registers the DLL and adds Q注音 to the input method list.

$ErrorActionPreference = 'Continue'
$dll = 'C:\QBopomofo\qbopomofo_tip.dll'
$logFile = 'C:\QBopomofo\sandbox-setup.log'
$tipId = '0404:{A7E3B4C1-9F2D-4E5A-B8C6-1D3F5A7E9B2C}{B8D1E2F3-6A4C-5D7E-9F0A-2B4C6D8E0F1A}'
$clsid = '{A7E3B4C1-9F2D-4E5A-B8C6-1D3F5A7E9B2C}'

# Log to file so we can debug if something goes wrong
function Log($msg) {
    $line = "$(Get-Date -Format 'HH:mm:ss') $msg"
    Write-Host $line
    Add-Content -Path $logFile -Value $line
}

function CopyRuntimeLogs {
    $runtimeLogs = @(
        (Join-Path $env:TEMP 'qbopomofo.log'),
        (Join-Path $env:TEMP 'qbopomofo_crash.log')
    )
    foreach ($runtimeLog in $runtimeLogs) {
        if (Test-Path $runtimeLog) {
            $dest = Join-Path 'C:\QBopomofo' (Split-Path $runtimeLog -Leaf)
            Copy-Item $runtimeLog $dest -Force -ErrorAction SilentlyContinue
            Log "[*] Copied runtime log: $dest"
        }
    }
}

Log '[*] Setting display resolution to 1280x800...'
Add-Type @"
using System;
using System.Runtime.InteropServices;

public class Display {
    [StructLayout(LayoutKind.Sequential)]
    public struct DEVMODE {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
        public short dmSpecVersion, dmDriverVersion;
        public short dmSize, dmDriverExtra;
        public int dmFields;
        public int dmPositionX, dmPositionY;
        public int dmDisplayOrientation, dmDisplayFixedOutput;
        public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
        public short dmLogPixels;
        public int dmBitsPerPel, dmPelsWidth, dmPelsHeight;
        public int dmDisplayFlags, dmDisplayFrequency;
        public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType;
        public int dmReserved1, dmReserved2, dmPanningWidth, dmPanningHeight;
    }
    [DllImport("user32.dll")] public static extern int EnumDisplaySettings(string dev, int mode, ref DEVMODE dm);
    [DllImport("user32.dll")] public static extern int ChangeDisplaySettings(ref DEVMODE dm, int flags);

    public static void SetRes(int w, int h) {
        var dm = new DEVMODE();
        dm.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE));
        EnumDisplaySettings(null, -1, ref dm);
        dm.dmPelsWidth = w;
        dm.dmPelsHeight = h;
        dm.dmFields = 0x80000 | 0x100000; // DM_PELSWIDTH | DM_PELSHEIGHT
        ChangeDisplaySettings(ref dm, 0);
    }
}
"@
[Display]::SetRes(800, 600)

Log '[*] Waiting for sandbox to settle...'
Start-Sleep 5

Log "[*] User: $env:USERNAME"
Log "[*] Package files:"
Get-ChildItem 'C:\QBopomofo' -File -ErrorAction SilentlyContinue |
    Sort-Object Name |
    ForEach-Object { Log "  $($_.Name) $($_.Length) bytes" }

if (Test-Path 'C:\QBopomofo\sandbox-package.txt') {
    Log '[*] Package manifest:'
    Get-Content 'C:\QBopomofo\sandbox-package.txt' | ForEach-Object { Log "  $_" }
}

# Check DLL exists
if (-not (Test-Path $dll)) {
    Log "[!] DLL not found at $dll"
    Log "[!] Available files in C:\QBopomofo:"
    Get-ChildItem 'C:\QBopomofo' -ErrorAction SilentlyContinue | ForEach-Object { Log "  $_" }
    pause
    exit 1
}

Log "[*] Registering DLL: $dll"
$env:QBOPOMOFO_DEBUG = '1'
$p = Start-Process regsvr32 -ArgumentList "/s `"$dll`"" -Wait -PassThru
Log "[*] regsvr32 exit code: $($p.ExitCode)"
CopyRuntimeLogs
if ($p.ExitCode -ne 0) {
    Log '[!] regsvr32 failed! Trying without /s for error message...'
    Start-Process regsvr32 -ArgumentList "`"$dll`"" -Wait
    CopyRuntimeLogs
    pause
    exit 1
}

Log '[*] COM registration check:'
$inproc = "Registry::HKEY_CLASSES_ROOT\CLSID\$clsid\InprocServer32"
if (Test-Path $inproc) {
    $props = Get-ItemProperty $inproc
    Log "  InprocServer32=$($props.'(default)')"
    Log "  ThreadingModel=$($props.ThreadingModel)"
} else {
    Log "  Missing $inproc"
}

# Add zh-Hant-TW language and Q注音 TIP
Log '[*] Adding Q注音 to input methods...'

try {
    $list = Get-WinUserLanguageList
    Log "[*] Current languages: $($list | ForEach-Object { $_.LanguageTag })"

    $lang = $list | Where-Object { $_.LanguageTag -eq 'zh-Hant-TW' }
    if (-not $lang) {
        Log '[*] zh-Hant-TW not found, adding...'
        $list.Add('zh-Hant-TW')
        Set-WinUserLanguageList $list -Force
        Start-Sleep 3
        $list = Get-WinUserLanguageList
        $lang = $list | Where-Object { $_.LanguageTag -eq 'zh-Hant-TW' }
    }

    if ($lang) {
        Log "[*] Current TIPs: $($lang.InputMethodTips -join ', ')"
        if ($lang.InputMethodTips -notcontains $tipId) {
            $lang.InputMethodTips.Add($tipId)
            Set-WinUserLanguageList $list -Force
            Log '[*] Q注音 added.'
            Start-Sleep 2
        } else {
            Log '[*] Q注音 already in list.'
        }
    } else {
        Log '[!] Could not find zh-Hant-TW language.'
    }
} catch {
    Log "[!] Error: $_"
}

try {
    $list = Get-WinUserLanguageList
    Log '[*] Final language list:'
    foreach ($item in $list) {
        Log "  $($item.LanguageTag): $($item.InputMethodTips -join ', ')"
    }
} catch {
    Log "[!] Could not read final language list: $_"
}

# Restart ctfmon
Log '[*] Restarting ctfmon...'
Stop-Process -Name ctfmon -Force -ErrorAction SilentlyContinue
Start-Sleep 2
Start-Process ctfmon.exe
Start-Sleep 1
$ctf = Get-Process -Name ctfmon -ErrorAction SilentlyContinue
Log "[*] ctfmon running: $([bool]$ctf)"

Log '[*] Done! Use Win+Space to switch to Q注音.'
Log "[*] Log saved to $logFile"
CopyRuntimeLogs

# Open Notepad for testing
Start-Process notepad.exe

# Keep window open so user can see output
Start-Sleep 3
