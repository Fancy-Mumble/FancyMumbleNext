<#
.SYNOPSIS
    Check Android development prerequisites and optionally launch the dev server.
.DESCRIPTION
    Validates that all required tools (JDK, Android SDK, NDK, Rust targets,
    Tauri CLI, emulator) are properly configured for Fancy Mumble Android
    development. Pass -Run to also start the dev server. Pass -Inspect to
    refresh WebView DevTools forwarding for chrome://inspect. Pass -Emulator
    to start an AVD emulator (optionally combined with -Run).
.EXAMPLE
    .\android-dev.ps1          # Check prerequisites only
.EXAMPLE
    .\android-dev.ps1 -Run     # Check prerequisites and start dev server
.EXAMPLE
    .\android-dev.ps1 -Emulator           # Launch emulator (pick AVD interactively)
.EXAMPLE
    .\android-dev.ps1 -Emulator -Run      # Launch emulator, then start dev server
.EXAMPLE
    .\android-dev.ps1 -Inspect # Set up WebView debugging for all devices
.EXAMPLE
    .\android-dev.ps1 -Inspect -Serial emulator-5554 # Target a specific device
.EXAMPLE
    .\android-dev.ps1 -Crash  # Dump and symbolize the latest native crash
.EXAMPLE
    .\android-dev.ps1 -Crash -Serial emulator-5554 # Target a specific device
#>
param(
    [switch]$Run,
    [switch]$Inspect,
    [switch]$Emulator,
    [switch]$Crash,
    [string]$Serial  # Target a specific ADB device serial (used with -Inspect / -Crash)
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

function Dump-CrashInfo {
    <#
    .SYNOPSIS
        Pull the latest native crash info (tombstone/logcat) from a connected
        device and symbolize it with ndk-stack.
    #>
    param(
        [string]$Serial,           # optional: target a specific device
        [string]$PackageName = "com.fancymumble.app"
    )

    $adbCmd = Get-Command adb -ErrorAction SilentlyContinue
    if (-not $adbCmd) {
        Write-Host "  [!!] adb not found in PATH." -ForegroundColor Red
        return
    }

    # Resolve device serial
    $adbArgs = @()
    if ($Serial) {
        $adbArgs = @('-s', $Serial)
    } else {
        $devLines = @(adb devices 2>$null | Select-String "^(\S+)\s+device\b")
        if ($devLines.Count -eq 0) {
            Write-Host "  [!!] No connected ADB device found." -ForegroundColor Red
            return
        }
        $Serial = $devLines[0].Matches[0].Groups[1].Value
        $adbArgs = @('-s', $Serial)
    }

    Write-Host "  Device: $Serial" -ForegroundColor DarkGray

    # Locate ndk-stack
    $ndkHome = if ($env:NDK_HOME) { $env:NDK_HOME } elseif ($env:ANDROID_NDK_HOME) { $env:ANDROID_NDK_HOME } else { $null }
    $ndkStackCmd = Get-Command ndk-stack -ErrorAction SilentlyContinue
    $ndkStackPath = $null
    if ($ndkStackCmd) {
        $ndkStackPath = $ndkStackCmd.Source
    } elseif ($ndkHome -and (Test-Path "$ndkHome\ndk-stack.cmd")) {
        $ndkStackPath = "$ndkHome\ndk-stack.cmd"
    } elseif ($ndkHome -and (Test-Path "$ndkHome\ndk-stack")) {
        $ndkStackPath = "$ndkHome\ndk-stack"
    }

    # Locate unstripped .so directory (for symbol resolution)
    $symDir = $null
    foreach ($profile in @('debug', 'release')) {
        $candidate = "$PSScriptRoot\..\target\aarch64-linux-android\$profile"
        if (Test-Path "$candidate\libmumble_tauri.so") {
            $symDir = (Resolve-Path $candidate).Path
            break
        }
    }

    # ---- 1. Logcat crash dump ----
    Write-Host "`n  --- Logcat crash output ---" -ForegroundColor Cyan
    $logcatRaw = & adb @adbArgs logcat -d -b crash 2>$null
    # Filter to lines about our package
    $crashLines = @($logcatRaw | Where-Object {
        $_ -match $PackageName -or $_ -match 'FATAL|SIGSEGV|SIGABRT|SIGBUS|SIGFPE|backtrace|signal \d+'
    })
    if ($crashLines.Count -gt 0) {
        $crashLines | ForEach-Object { Write-Host "    $_" -ForegroundColor Yellow }
    } else {
        Write-Host "    (no crash entries in logcat)" -ForegroundColor DarkGray
    }

    # ---- 2. Tombstone via ndk-stack ----
    if ($ndkStackPath -and $symDir) {
        Write-Host "`n  --- Symbolized backtrace (ndk-stack) ---" -ForegroundColor Cyan
        Write-Host "    Symbols: $symDir" -ForegroundColor DarkGray
        Write-Host "    ndk-stack: $ndkStackPath" -ForegroundColor DarkGray

        # Pipe logcat crash buffer through ndk-stack for symbolication
        $logcatAll = & adb @adbArgs logcat -d 2>$null
        $symbolized = $logcatAll | & $ndkStackPath -sym $symDir 2>$null
        $symbolized = @($symbolized | Where-Object { $_ })
        if ($symbolized.Count -gt 0) {
            $symbolized | ForEach-Object { Write-Host "    $_" -ForegroundColor White }
        } else {
            Write-Host "    (ndk-stack produced no output - no native backtrace found)" -ForegroundColor DarkGray
        }
    } elseif (-not $ndkStackPath) {
        Write-Host "`n  [!!] ndk-stack not found. Set NDK_HOME for symbolized backtraces." -ForegroundColor Yellow
    } elseif (-not $symDir) {
        Write-Host "`n  [!!] No unstripped .so found in target/. Build for Android first." -ForegroundColor Yellow
    }

    # ---- 3. Tombstone files (Android 12+ bugreport-style) ----
    Write-Host "`n  --- Device tombstones ---" -ForegroundColor Cyan
    $tombstoneList = & adb @adbArgs shell ls /data/tombstones/ 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $tombstoneList) {
        # Non-root devices may not have access, try via dumpsys
        $tombstoneList = & adb @adbArgs shell dumpsys dropbox --print SYSTEM_TOMBSTONE 2>$null |
            Select-Object -Last 80
        if ($tombstoneList) {
            Write-Host "    (via dumpsys dropbox SYSTEM_TOMBSTONE, last 80 lines):" -ForegroundColor DarkGray
            $tombstoneList | ForEach-Object { Write-Host "    $_" -ForegroundColor White }
        } else {
            Write-Host "    (no tombstone access - device may require root)" -ForegroundColor DarkGray
        }
    } else {
        $tombFiles = @($tombstoneList -split "`n" | Where-Object { $_ -match 'tombstone' } | Sort-Object)
        if ($tombFiles.Count -gt 0) {
            $latest = $tombFiles[-1].Trim()
            Write-Host "    Latest: $latest" -ForegroundColor DarkGray
            $tombContent = & adb @adbArgs shell cat "/data/tombstones/$latest" 2>$null
            if ($tombContent) {
                if ($ndkStackPath -and $symDir) {
                    $tombSymbolized = $tombContent | & $ndkStackPath -sym $symDir 2>$null
                    $tombSymbolized | ForEach-Object { Write-Host "    $_" -ForegroundColor White }
                } else {
                    $tombContent | Select-Object -First 80 | ForEach-Object { Write-Host "    $_" -ForegroundColor White }
                }
            } else {
                Write-Host "    (could not read tombstone - permission denied?)" -ForegroundColor DarkGray
            }
        } else {
            Write-Host "    (no tombstone files found)" -ForegroundColor DarkGray
        }
    }
    Write-Host ""
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

    # cmake-rs does not auto-set ANDROID_ABI when the toolchain file comes
    # from an env var (only when .define() is called in code). We use
    # per-target wrapper files that pre-set the correct ABI before
    # including the real NDK toolchain.
    $scriptDir = $PSScriptRoot
    $toolchainMap = @{
        "aarch64_linux_android"    = "$scriptDir\cmake\aarch64-android.toolchain.cmake"
        "x86_64_linux_android"     = "$scriptDir\cmake\x86_64-android.toolchain.cmake"
        "armv7_linux_androideabi"   = "$scriptDir\cmake\armv7-android.toolchain.cmake"
        "i686_linux_android"        = "$scriptDir\cmake\i686-android.toolchain.cmake"
    }
    foreach ($triple in $toolchainMap.Keys) {
        $wrapper = $toolchainMap[$triple]
        if (Test-Path $wrapper) {
            Set-Item "env:CMAKE_TOOLCHAIN_FILE_$triple" $wrapper
        } elseif (Test-Path $androidToolchain) {
            # Fallback to raw NDK toolchain if wrapper is missing
            Set-Item "env:CMAKE_TOOLCHAIN_FILE_$triple" $androidToolchain
        }
        if (Test-Path $ndkNinjaExe) {
            Set-Item "env:CMAKE_MAKE_PROGRAM_$triple" $ndkNinjaExe
        }
    }

    # Set CC/CXX/AR and linker for each Android target so that raw
    # `cargo build --target <triple>` works (not only cargo-tauri).
    # The NDK provides target-prefixed clang wrappers in the LLVM bin dir.
    $llvmBin = "$ndkHome\toolchains\llvm\prebuilt\windows-x86_64\bin"
    $ccMap = @{
        "aarch64_linux_android"  = @{ clang = "aarch64-linux-android24-clang";  cargo = "AARCH64_LINUX_ANDROID" }
        "x86_64_linux_android"   = @{ clang = "x86_64-linux-android24-clang";   cargo = "X86_64_LINUX_ANDROID" }
        "armv7_linux_androideabi"= @{ clang = "armv7a-linux-androideabi24-clang"; cargo = "ARMV7_LINUX_ANDROIDEABI" }
        "i686_linux_android"     = @{ clang = "i686-linux-android24-clang";     cargo = "I686_LINUX_ANDROID" }
    }
    foreach ($triple in $ccMap.Keys) {
        $info = $ccMap[$triple]
        $cc  = "$llvmBin\$($info.clang).cmd"
        $cxx = "$llvmBin\$($info.clang)++.cmd"
        $ar  = "$llvmBin\llvm-ar.exe"
        if (Test-Path $cc) {
            Set-Item "env:CC_$triple" $cc
            Set-Item "env:CXX_$triple" $cxx
            Set-Item "env:AR_$triple" $ar
            Set-Item "env:CARGO_TARGET_$($info.cargo)_LINKER" $cc
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
    $devices = (adb devices 2>&1) | Select-String "^\S+\s+device\b"
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

if ($Emulator) {
    $emulatorExe = Get-Command emulator -ErrorAction SilentlyContinue
    if (-not $emulatorExe) {
        Write-Host "emulator not found in PATH. Install via Android Studio SDK Manager." -ForegroundColor Red
        exit 1
    }
    $avdList = @(emulator -list-avds 2>&1 | Where-Object { $_ -and $_ -notmatch "^(INFO|WARNING)" })
    if ($avdList.Count -eq 0) {
        Write-Host "No AVDs found. Create one in Android Studio > Device Manager." -ForegroundColor Red
        exit 1
    }
    if ($avdList.Count -eq 1) {
        $avdName = $avdList[0]
    } else {
        Write-Host "`nAvailable AVDs:" -ForegroundColor Cyan
        for ($i = 0; $i -lt $avdList.Count; $i++) {
            Write-Host "  [$($i + 1)] $($avdList[$i])" -ForegroundColor White
        }
        $choice = Read-Host "`nSelect AVD (1-$($avdList.Count))"
        $idx = 0
        if (-not [int]::TryParse($choice, [ref]$idx) -or $idx -lt 1 -or $idx -gt $avdList.Count) {
            Write-Host "Invalid selection." -ForegroundColor Red
            exit 1
        }
        $avdName = $avdList[$idx - 1]
    }
    Write-Host "`nLaunching emulator: $avdName ..." -ForegroundColor Cyan
    Start-Process -FilePath $emulatorExe.Source -ArgumentList @("-avd", $avdName) -WindowStyle Minimized
    # Wait for the emulator to become ready
    Write-Host "Waiting for emulator to boot..." -ForegroundColor DarkGray
    $timeout = 120
    $elapsed = 0
    $booted = $false
    $emulatorSerial = $null
    # Phase 1: wait for the new emulator to appear in adb devices
    while ($elapsed -lt $timeout -and -not $emulatorSerial) {
        Start-Sleep -Seconds 2
        $elapsed += 2
        $emulatorSerial = adb devices 2>$null |
            Select-String "^(emulator-\d+)\s+device" |
            ForEach-Object { $_.Matches[0].Groups[1].Value } |
            Select-Object -Last 1
    }
    # Phase 2: poll sys.boot_completed on the detected emulator
    if ($emulatorSerial) {
        while ($elapsed -lt $timeout) {
            $bootVal = adb -s $emulatorSerial shell getprop sys.boot_completed 2>$null
            if ("$bootVal".Trim() -eq "1") {
                $booted = $true
                break
            }
            Start-Sleep -Seconds 2
            $elapsed += 2
        }
    }
    if ($booted) {
        Write-Host "  [OK] Emulator booted ($avdName)" -ForegroundColor Green
    } else {
        Write-Host "  [!!] Emulator did not finish booting within ${timeout}s." -ForegroundColor Yellow
        Write-Host "       Continuing anyway - it may still be starting up." -ForegroundColor Yellow
    }
    Write-Host ""
}

if ($Run) {
    Write-Host "`nStarting Tauri Android dev server..." -ForegroundColor Cyan
    Push-Location "$PSScriptRoot\..\crates\mumble-tauri"
    try {
        # --no-watch: disable the Tauri file watcher so frontend file changes
        # (handled by Vite HMR) do not trigger expensive full Rust+Gradle
        # rebuilds. Without this the watcher monitors ui/src/** and causes
        # a rebuild loop on Android.
        cargo tauri android dev --no-watch
        $devExitCode = $LASTEXITCODE
    } finally {
        Pop-Location
    }

    # After the dev server exits, check for native crashes automatically
    if ($devExitCode -ne 0) {
        Write-Host "`n" -NoNewline
        Write-Host ("=" * 60) -ForegroundColor Red
        Write-Host "  Dev server exited with code $devExitCode - checking for native crashes..." -ForegroundColor Red
        Write-Host ("=" * 60) -ForegroundColor Red
        $dumpArgs = @{}
        if ($Serial) { $dumpArgs.Serial = $Serial }
        Dump-CrashInfo @dumpArgs
    }
} elseif ($Crash) {
    Write-Host "`nNative Crash Dump" -ForegroundColor Cyan
    Write-Host ("-" * 48)
    $dumpArgs = @{}
    if ($Serial) { $dumpArgs.Serial = $Serial }
    Dump-CrashInfo @dumpArgs
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
    Write-Host "Run with -Emulator to launch an emulator:" -ForegroundColor DarkGray
    Write-Host "  .\scripts\android-dev.ps1 -Emulator" -ForegroundColor DarkGray
    Write-Host "Run with -Emulator -Run to launch emulator and start dev server:" -ForegroundColor DarkGray
    Write-Host "  .\scripts\android-dev.ps1 -Emulator -Run" -ForegroundColor DarkGray
    Write-Host "Run with -Crash to dump and symbolize the latest native crash:" -ForegroundColor DarkGray
    Write-Host "  .\scripts\android-dev.ps1 -Crash" -ForegroundColor DarkGray
    Write-Host "Run with -Inspect to refresh chrome://inspect forwarding:" -ForegroundColor DarkGray
    Write-Host "  .\scripts\android-dev.ps1 -Inspect" -ForegroundColor DarkGray
}
