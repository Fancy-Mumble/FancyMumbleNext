#Requires -Version 5.1
<#
.SYNOPSIS
    Stream logcat output for com.fancymumble.app from an Android emulator/device.

.PARAMETER Serial
    ADB device serial (e.g. emulator-5554).  If omitted, auto-detects the first
    available emulator, then falls back to the only connected device.

.PARAMETER Package
    Android package name to filter.  Defaults to com.fancymumble.app.

.EXAMPLE
    .\scripts\android-logcat.ps1
    .\scripts\android-logcat.ps1 -Serial emulator-5554
#>
param(
    [string]$Serial,
    [string]$Package = "com.fancymumble.app"
)

# Auto-detect serial when not specified
if (-not $Serial) {
    $deviceLines = @(adb devices 2>$null | Select-Object -Skip 1 | Where-Object { $_ -match "\S" })
    $emulator = $deviceLines | Where-Object { $_ -match "^emulator-\d+\s+device" } | Select-Object -First 1
    if ($emulator -match "^(\S+)") {
        $Serial = $Matches[1]
    } else {
        $any = $deviceLines | Where-Object { $_ -match "^(\S+)\s+device" } | Select-Object -First 1
        if ($any -match "^(\S+)") {
            $Serial = $Matches[1]
        }
    }
    if (-not $Serial) {
        Write-Host "No authorized ADB device or emulator found. Start one first." -ForegroundColor Red
        exit 1
    }
    Write-Host "Auto-selected device: $Serial" -ForegroundColor DarkGray
}

# Resolve the app PID (use $appPid to avoid the read-only $PID built-in)
$appPid = (adb -s $Serial shell pidof $Package 2>$null).Trim()

if (-not $appPid) {
    Write-Host "Package '$Package' is not running on $Serial." -ForegroundColor Yellow
    Write-Host "Showing all logs (no PID filter). Launch the app to get PID-filtered output." -ForegroundColor DarkGray
    adb -s $Serial logcat
} else {
    Write-Host "Streaming logcat for $Package (PID $appPid) on $Serial ..." -ForegroundColor Cyan
    Write-Host "Press Ctrl+C to stop." -ForegroundColor DarkGray
    adb -s $Serial logcat --pid=$appPid
}
