<#
.SYNOPSIS
    Check Android development prerequisites and optionally launch the dev server.
.DESCRIPTION
    Validates that all required tools (JDK, Android SDK, NDK, Rust targets,
    Tauri CLI, emulator) are properly configured for Fancy Mumble Android
    development. Pass -Run to also start the dev server.
.EXAMPLE
    .\android-dev.ps1          # Check prerequisites only
    .\android-dev.ps1 -Run     # Check prerequisites and start dev server
#>
param(
    [switch]$Run
)

$ErrorActionPreference = "Continue"
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
    $javaVer = & "$javaHome\bin\java.exe" -version 2>&1 | Select-Object -First 1
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
    # CMake's Android platform module looks for ANDROID_NDK_HOME, not NDK_HOME.
    # Set it for this session so CMake can locate the NDK toolchain.
    if (-not $env:ANDROID_NDK_HOME) { $env:ANDROID_NDK_HOME = $ndkHome }

    # On Windows, CMake cannot discover the NDK or ninja automatically.
    # Set CMAKE_TOOLCHAIN_FILE_<target> to the NDK's bundled toolchain file
    # and CMAKE_MAKE_PROGRAM_<target> to the NDK's bundled ninja for every
    # Android target triple (hyphens replaced with underscores as the cmake
    # crate requires).
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

    # Also add NDK-bundled ninja dir to PATH as a fallback.
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
# Auto-add platform-tools and emulator to PATH for this session when ANDROID_HOME is known
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
    exit 0
}

Write-Host "All prerequisites satisfied!" -ForegroundColor Green

if ($Run) {
    Write-Host "`nStarting Tauri Android dev server..." -ForegroundColor Cyan
    Push-Location "$PSScriptRoot\..\crates\mumble-tauri"
    try {
        cargo tauri android dev
    } finally {
        Pop-Location
    }
} else {
    Write-Host "Run with -Run to start the dev server, or manually:" -ForegroundColor DarkGray
    Write-Host "  cd crates\mumble-tauri" -ForegroundColor DarkGray
    Write-Host "  cargo tauri android dev" -ForegroundColor DarkGray
}
