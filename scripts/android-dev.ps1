<#
.SYNOPSIS
    Check Android development prerequisites and optionally launch the dev server.
.DESCRIPTION
    Validates that all required tools (JDK, Android SDK, NDK, Rust targets,
    Tauri CLI, emulator) are properly configured for Fancy Mumble Android
    development. Pass -Run to also start the dev server. Pass -Inspect to
    refresh WebView DevTools forwarding for chrome://inspect.
.EXAMPLE
    .\android-dev.ps1          # Check prerequisites only
.EXAMPLE
    .\android-dev.ps1 -Run     # Check prerequisites and start dev server
.EXAMPLE
    .\android-dev.ps1 -Inspect # Set up WebView debugging for all devices
.EXAMPLE
    .\android-dev.ps1 -Inspect -Serial emulator-5554 # Target a specific device
#>
param(
    [switch]$Run,
    [switch]$Inspect,
    [string]$Serial  # Target a specific ADB device serial (used with -Inspect)
)

$ErrorActionPreference = "Continue"
# Prevent PowerShell 7+ from treating non-zero native exit codes as errors
if (Test-Path variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}
$script:hasErrors = $false

function Write-Check {
    param([string]$Label, [bool]$Ok, [string]$Detail = "")
    if ($Ok) {
        Write-Host "  [OK] $Label" -ForegroundColor Green
        if ($Detail) { Write-Host "       $Detail" -ForegroundColor DarkGray }
    } else {
        Write-Host "  [!!] $Label" -ForegroundColor Red
        if ($Detail) { Write-Host "       $Detail" -ForegroundColor Yellow }
        $script:hasErrors = $true
    }
}

function Start-WebViewInspect {
    param(
        [string]$PackageName = "com.fancymumble.app",
        [int]$BasePort = 9222,
        [string]$Serial  # optional: target a specific device serial
    )

    # Helper: run adb and ignore non-zero exit codes
    function Invoke-Adb {
        param([string[]]$Arguments)
        $output = & adb @Arguments 2>$null
        $global:LASTEXITCODE = 0
        return $output
    }

    $adbCmd = Get-Command adb -ErrorAction SilentlyContinue
    if (-not $adbCmd) {
        Write-Host "  [!!] adb not found in PATH." -ForegroundColor Red
        return $false
    }

    # Enumerate all connected devices
    $deviceRows = @(Invoke-Adb -Arguments @('devices', '-l') |
        Select-Object -Skip 1 |
        Where-Object { $_ -match "\S" })
    $allDevices = [System.Collections.ArrayList]::new()
    foreach ($row in $deviceRows) {
        if ($row -match "^(\S+)\s+device\b") {
            $devSerial = $Matches[1].Trim()
            $devModel = if ($row -match "model:(\S+)") { $Matches[1] } else { "unknown" }
            $devKind = if ($devSerial -like "emulator-*") { "emulator" } else { "usb" }
            [void]$allDevices.Add([PSCustomObject]@{
                Serial = $devSerial
                Model  = $devModel
                Kind   = $devKind
            })
        } elseif ($row -match "^(\S+)\s+unauthorized") {
            Write-Host "  [!!] $($Matches[1]) is unauthorized - accept the USB debugging prompt on the device" -ForegroundColor Yellow
        } elseif ($row -match "^(\S+)\s+offline") {
            Write-Host "  [!!] $($Matches[1]) is offline - reconnect cable or restart ADB" -ForegroundColor Yellow
        }
    }

    if ($allDevices.Count -eq 0) {
        Write-Host "  [!!] No authorized ADB device or emulator found." -ForegroundColor Red
        Write-Host "       - For emulators: start one from Android Studio Device Manager" -ForegroundColor Yellow
        Write-Host "       - For USB devices: enable USB debugging in Developer Options" -ForegroundColor Yellow
        return $false
    }

    # Filter to a specific serial if requested
    $targets = $allDevices
    if ($Serial) {
        $targets = @($allDevices | Where-Object { $_.Serial -eq $Serial })
        if ($targets.Count -eq 0) {
            Write-Host "  [!!] Device '$Serial' not found. Available:" -ForegroundColor Red
            foreach ($d in $allDevices) {
                Write-Host "       $($d.Serial) ($($d.Kind), $($d.Model))" -ForegroundColor Yellow
            }
            return $false
        }
    }

    Write-Host "  Found $($targets.Count) device(s):" -ForegroundColor Cyan
    foreach ($d in $targets) {
        Write-Host "    - $($d.Serial) [$($d.Kind)] $($d.Model)" -ForegroundColor DarkGray
    }
    Write-Host ""

    # Process each device: check app, find sockets, set up forwarding
    $port = $BasePort
    $anyForwarded = $false
    $forwardedDevices = [System.Collections.ArrayList]::new()

    foreach ($device in $targets) {
        $s = $device.Serial
        Write-Host "  --- $s ($($device.Kind), $($device.Model)) ---" -ForegroundColor Cyan

        # Check if the app is running
        $pidLine = Invoke-Adb -Arguments @('-s', $s, 'shell', 'pidof', $PackageName) |
            Select-Object -First 1
        $appPid = if ($null -ne $pidLine) { "$pidLine".Trim() } else { "" }

        if ([string]::IsNullOrWhiteSpace($appPid)) {
            Write-Host "  [--] App not running. Start it first, then re-run -Inspect." -ForegroundColor DarkGray
            Write-Host ""
            continue
        }

        Write-Host "  [OK] App running (PID $appPid)" -ForegroundColor Green

        # Check for WebView DevTools sockets
        $unixOutput = Invoke-Adb -Arguments @('-s', $s, 'shell', 'cat /proc/net/unix')
        $socketLines = @($unixOutput | Select-String "webview_devtools_remote")

        if ($socketLines.Count -eq 0) {
            Write-Host "  [!!] No WebView DevTools socket found." -ForegroundColor Yellow
            Write-Host "       The WebView may not have loaded yet, or debugging is disabled." -ForegroundColor Yellow
            Write-Host "       - Ensure you are running a debug build (cargo tauri android dev)" -ForegroundColor Yellow
            Write-Host "       - Wait for the WebView to fully load, then re-run -Inspect" -ForegroundColor Yellow
            Write-Host ""
            continue
        }

        # Extract the socket name matching this PID
        $socketName = "webview_devtools_remote_$appPid"
        $matchedSocket = $socketLines | Where-Object { $_.Line -match $socketName }

        if (-not $matchedSocket) {
            # Fall back to any devtools socket found
            $fallbackMatch = $socketLines | Select-Object -First 1
            if ($fallbackMatch -and $fallbackMatch.Line -match "@(webview_devtools_remote_\d+)") {
                $socketName = $Matches[1]
                Write-Host "  [!!] PID mismatch - using discovered socket: $socketName" -ForegroundColor Yellow
            } else {
                Write-Host "  [!!] Could not match DevTools socket to app PID." -ForegroundColor Yellow
                Write-Host ""
                continue
            }
        }

        Write-Host "  [OK] DevTools socket: $socketName" -ForegroundColor Green

        # Set up port forwarding
        Invoke-Adb -Arguments @('-s', $s, 'forward', '--remove', "tcp:$port") | Out-Null
        Invoke-Adb -Arguments @('-s', $s, 'forward', "tcp:$port", "localabstract:$socketName") | Out-Null
        Start-Sleep -Milliseconds 300

        # Verify the forwarding works
        try {
            $null = Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$port/json/version" -TimeoutSec 3
            Write-Host "  [OK] Forwarded tcp:$port -> $socketName" -ForegroundColor Green
            Write-Host "       Direct URL: http://127.0.0.1:$port/json/list" -ForegroundColor DarkGray
            $anyForwarded = $true
            $forwardedDevices += [PSCustomObject]@{ Serial = $s; Port = $port }
        } catch {
            Write-Host "  [!!] Forwarding on port $port failed. Try: adb kill-server; adb start-server" -ForegroundColor Yellow
        }

        Write-Host ""
        $port++
    }

    # Summary and chrome://inspect guidance
    Write-Host "  === How to inspect ===" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "  Option 1 - chrome://inspect (recommended):" -ForegroundColor White
    Write-Host "    1. Open Chrome and navigate to chrome://inspect/#devices" -ForegroundColor DarkGray
    Write-Host "    2. Ensure 'Discover USB devices' is checked" -ForegroundColor DarkGray
    Write-Host "    3. Your device(s) and WebView(s) should appear automatically" -ForegroundColor DarkGray
    Write-Host ""

    if ($anyForwarded) {
        Write-Host "  Option 2 - Direct DevTools (port-forwarded above):" -ForegroundColor White
        foreach ($fd in $forwardedDevices) {
            Write-Host "    $($fd.Serial) -> http://127.0.0.1:$($fd.Port)/json/list" -ForegroundColor DarkGray
        }
        Write-Host ""
    }

    # Troubleshooting hints
    Write-Host "  Troubleshooting:" -ForegroundColor Yellow
    Write-Host "    - Device not visible? Run: adb kill-server; adb start-server" -ForegroundColor DarkGray
    Write-Host "    - USB device unauthorized? Accept the debugging prompt on the device" -ForegroundColor DarkGray
    Write-Host "    - No WebView socket? Ensure you run a debug build (cargo tauri android dev)" -ForegroundColor DarkGray
    Write-Host "    - Target a specific device: .\android-dev.ps1 -Inspect -Serial emulator-5554" -ForegroundColor DarkGray
    Write-Host ""

    return $anyForwarded -or ($targets.Count -gt 0)
}

Write-Host "`nFancy Mumble - Android Dev Environment Check" -ForegroundColor Cyan
Write-Host ("=" * 48)

# -- Windows Developer Mode (required for symlink creation by Tauri) --
$devModeKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock"
$devModeValue = (Get-ItemProperty -Path $devModeKey -Name "AllowDevelopmentWithoutDevLicense" -ErrorAction SilentlyContinue).AllowDevelopmentWithoutDevLicense
if ($devModeValue -eq 1) {
    Write-Check "Developer Mode" $true "Symlink creation enabled"
} else {
    Write-Check "Developer Mode" $false "Required for Tauri Android symlinks. Enable via: Start-Process 'ms-settings:developers'"
}

# -- JAVA_HOME / JDK --
# Auto-detect from common install locations when not set
$javaHome = $env:JAVA_HOME
if (-not ($javaHome -and (Test-Path "$javaHome\bin\java.exe"))) {
    $candidatePaths = @(
        # Android Studio bundled JBR (default install path)
        "C:\Program Files\Android\Android Studio\jbr",
        # Android Studio in user AppData
        "$env:LOCALAPPDATA\Programs\Android Studio\jbr",
        # Eclipse Temurin / Adoptium JDK 17 & 21
        "C:\Program Files\Eclipse Adoptium\jdk-21*\*",
        "C:\Program Files\Eclipse Adoptium\jdk-17*\*",
        # Oracle / OpenJDK
        "C:\Program Files\Java\jdk-21*",
        "C:\Program Files\Java\jdk-17*",
        "C:\Program Files\Microsoft\jdk-17*\*"
    )
    foreach ($pattern in $candidatePaths) {
        $found = Resolve-Path $pattern -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found -and (Test-Path "$found\bin\java.exe")) {
            $javaHome = $found.Path
            $env:JAVA_HOME = $javaHome
            break
        }
    }
}

if ($javaHome -and (Test-Path "$javaHome\bin\java.exe")) {
    $null = & "$javaHome\bin\java.exe" -version 2>&1 | Select-Object -First 1
    $autoNote = if ($env:JAVA_HOME -eq $javaHome -and -not [System.Environment]::GetEnvironmentVariable('JAVA_HOME', 'User')) { " (auto-detected for this session)" } else { "" }
    Write-Check "JAVA_HOME" $true "$javaHome$autoNote"
} else {
    Write-Check "JAVA_HOME" $false @"
Not found. To fix, run one of:
  - Open Android Studio > Settings > Build Tools > Gradle and note the Gradle JDK path
  - [System.Environment]::SetEnvironmentVariable('JAVA_HOME', 'C:\Program Files\Android\Android Studio\jbr', 'User')
"@
}

# -- ANDROID_HOME --
$androidHome = $env:ANDROID_HOME
if ($androidHome -and (Test-Path $androidHome)) {
    Write-Check "ANDROID_HOME" $true $androidHome
} else {
    Write-Check "ANDROID_HOME" $false "Set ANDROID_HOME to the Android SDK path"
}

# -- NDK_HOME --
$ndkHome = $env:NDK_HOME
if ($ndkHome -and (Test-Path $ndkHome)) {
    if (-not $env:ANDROID_NDK_HOME) { $env:ANDROID_NDK_HOME = $ndkHome }

    $androidToolchain = "$ndkHome\build\cmake\android.toolchain.cmake"
    $ndkNinjaExe      = "$ndkHome\prebuilt\windows-x86_64\bin\ninja.exe"
    foreach ($triple in @(
        "x86_64_linux_android",
        "aarch64_linux_android",
        "armv7_linux_androideabi",
        "i686_linux_android"
    )) {
        if (Test-Path $androidToolchain) {
            Set-Item "env:CMAKE_TOOLCHAIN_FILE_$triple" $androidToolchain
        }
        if (Test-Path $ndkNinjaExe) {
            Set-Item "env:CMAKE_MAKE_PROGRAM_$triple" $ndkNinjaExe
        }
    }

    $ndkNinjaDir = "$ndkHome\prebuilt\windows-x86_64\bin"
    $pathParts = $env:PATH -split ';'
    if ((Test-Path $ndkNinjaDir) -and ($pathParts -notcontains $ndkNinjaDir)) {
        $env:PATH = "$ndkNinjaDir;$env:PATH"
    }
    Write-Check "NDK_HOME" $true $ndkHome
} elseif ($androidHome) {
    $ndkDir = Get-ChildItem "$androidHome\ndk" -Directory -ErrorAction SilentlyContinue |
              Sort-Object Name -Descending | Select-Object -First 1
    if ($ndkDir) {
        Write-Check "NDK_HOME" $false "NDK found at $($ndkDir.FullName) but NDK_HOME is not set. Run: `$env:NDK_HOME = '$($ndkDir.FullName)'"
    } else {
        Write-Check "NDK_HOME" $false "Install NDK via Android Studio SDK Manager or: sdkmanager --install 'ndk;27.0.12077973'"
    }
} else {
    Write-Check "NDK_HOME" $false "Set ANDROID_HOME first, then install NDK"
}

# -- Rust Android targets --
$targets = rustup target list --installed 2>&1
$requiredTargets = @("aarch64-linux-android", "x86_64-linux-android")
$missingTargets = @()
foreach ($t in $requiredTargets) {
    if ($targets -notcontains $t) { $missingTargets += $t }
}
if ($missingTargets.Count -eq 0) {
    Write-Check "Rust Android targets" $true ($requiredTargets -join ", ")
} else {
    Write-Check "Rust Android targets" $false "Missing: $($missingTargets -join ', '). Run: rustup target add $($missingTargets -join ' ')"
}

# -- Tauri CLI --
$tauriCli = Get-Command cargo-tauri -ErrorAction SilentlyContinue
if ($tauriCli) {
    Write-Check "Tauri CLI" $true "cargo-tauri found"
} else {
    Write-Check "Tauri CLI" $false "Run: cargo install tauri-cli --version '^2'"
}

# -- ADB / Emulator --
if ($androidHome) {
    $platformTools = "$androidHome\platform-tools"
    $emulatorDir   = "$androidHome\emulator"
    $pathParts = $env:PATH -split ';'
    if ((Test-Path $platformTools) -and ($pathParts -notcontains $platformTools)) {
        $env:PATH = "$platformTools;$env:PATH"
    }
    if ((Test-Path $emulatorDir) -and ($pathParts -notcontains $emulatorDir)) {
        $env:PATH = "$emulatorDir;$env:PATH"
    }
}

$adb = Get-Command adb -ErrorAction SilentlyContinue
if ($adb) {
    $devices = (adb devices 2>&1) | Select-String "device$"
    $deviceCount = ($devices | Measure-Object).Count
    Write-Check "ADB" $true "$deviceCount device(s)/emulator(s) connected"
} else {
    Write-Check "ADB" $false "Add '$androidHome\platform-tools' to your PATH"
}

$emulatorCmd = Get-Command emulator -ErrorAction SilentlyContinue
if ($emulatorCmd) {
    $avds = emulator -list-avds 2>&1 | Where-Object { $_ -and $_ -notmatch "^(INFO|WARNING)" }
    $avdCount = ($avds | Measure-Object).Count
    if ($avdCount -gt 0) {
        Write-Check "Emulator AVDs" $true "$avdCount available: $($avds -join ', ')"
    } else {
        Write-Check "Emulator AVDs" $false "No AVDs found. Create one in Android Studio > Device Manager"
    }
} else {
    Write-Check "Emulator" $false "Emulator binary not found in '$androidHome\emulator'. Reinstall via Android Studio SDK Manager."
}

Write-Host ""

if ($script:hasErrors) {
    Write-Host "Some prerequisites are missing. See ANDROID_DEV.md for setup instructions." -ForegroundColor Yellow
    if ($Run) {
        Write-Host "Fix the issues above before running the dev server." -ForegroundColor Red
        exit 1
    }
    if (-not $Inspect) {
        exit 0
    }
    Write-Host "Continuing inspect mode despite prerequisite warnings..." -ForegroundColor Yellow
} else {
    Write-Host "All prerequisites satisfied!" -ForegroundColor Green
}

if ($Run) {
    Write-Host "`nStarting Tauri Android dev server..." -ForegroundColor Cyan
    Push-Location "$PSScriptRoot\..\crates\mumble-tauri"
    try {
        cargo tauri android dev
    } finally {
        Pop-Location
    }
} elseif ($Inspect) {
    Write-Host "`nWebView DevTools Inspect" -ForegroundColor Cyan
    Write-Host ("-" * 48)
    $inspectArgs = @{ PackageName = "com.fancymumble.app" }
    if ($Serial) { $inspectArgs.Serial = $Serial }
    [void](Start-WebViewInspect @inspectArgs)
} else {
    Write-Host "Run with -Run to start the dev server, or manually:" -ForegroundColor DarkGray
    Write-Host "  cd crates\mumble-tauri" -ForegroundColor DarkGray
    Write-Host "  cargo tauri android dev" -ForegroundColor DarkGray
    Write-Host "Run with -Inspect to refresh chrome://inspect forwarding:" -ForegroundColor DarkGray
    Write-Host "  .\scripts\android-dev.ps1 -Inspect" -ForegroundColor DarkGray
}
