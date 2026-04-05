<#
.SYNOPSIS
    Run the desktop and Android Tauri dev builds simultaneously.
.DESCRIPTION
    Starts a single shared Vite dev server, then launches both
    'cargo tauri dev' (desktop) and 'cargo tauri android dev' in
    parallel.  The desktop build runs in its own console window
    (required for WebView2 GUI), while Vite and Android run as
    background jobs with tagged output in the main console.

    You can also run only the desktop or only the Android build by
    passing -DesktopOnly or -AndroidOnly.
.EXAMPLE
    .\dual-dev.ps1                  # Desktop + Android
.EXAMPLE
    .\dual-dev.ps1 -DesktopOnly     # Desktop only (shared Vite)
.EXAMPLE
    .\dual-dev.ps1 -AndroidOnly     # Android only (shared Vite)
#>
param(
    [switch]$DesktopOnly,
    [switch]$AndroidOnly
)

$ErrorActionPreference = "Stop"

$repoRoot  = Split-Path $PSScriptRoot -Parent
$tauriDir  = Join-Path $repoRoot "crates\mumble-tauri"
$uiDir     = Join-Path $tauriDir "ui"
$bridgeDir = Join-Path $repoRoot "crates\signal-bridge"

# -------------------------------------------------------------------
# Helpers
# -------------------------------------------------------------------
function Write-Tag {
    param([string]$Tag, [ConsoleColor]$Color, [string]$Message)
    Write-Host "[$Tag] " -ForegroundColor $Color -NoNewline
    Write-Host $Message
}

function Resolve-NdkHome {
    if ($env:NDK_HOME -and (Test-Path $env:NDK_HOME)) { return $env:NDK_HOME }
    if ($env:ANDROID_NDK_HOME -and (Test-Path $env:ANDROID_NDK_HOME)) { return $env:ANDROID_NDK_HOME }
    $persistedNdk = [System.Environment]::GetEnvironmentVariable("NDK_HOME", "User")
    if ($persistedNdk -and (Test-Path $persistedNdk)) { return $persistedNdk }
    $androidHome = $env:ANDROID_HOME
    if (-not ($androidHome -and (Test-Path $androidHome))) {
        $persistedHome = [System.Environment]::GetEnvironmentVariable("ANDROID_HOME", "User")
        if ($persistedHome -and (Test-Path $persistedHome)) { $androidHome = $persistedHome }
        elseif (Test-Path "$env:LOCALAPPDATA\Android\Sdk") { $androidHome = "$env:LOCALAPPDATA\Android\Sdk" }
    }
    if ($androidHome) {
        $ndkDir = Get-ChildItem "$androidHome\ndk" -Directory -ErrorAction SilentlyContinue |
                  Sort-Object Name -Descending | Select-Object -First 1
        if ($ndkDir) { return $ndkDir.FullName }
    }
    return $null
}

function Build-SignalBridgeAndroid {
    <#
    .SYNOPSIS
        Cross-compile signal-bridge for Android and place the .so in jniLibs.
    #>
    param([string]$NdkHome, [string[]]$Targets)

    $llvmBin = "$NdkHome\toolchains\llvm\prebuilt\windows-x86_64\bin"
    if (-not (Test-Path $llvmBin)) {
        # Linux / macOS host
        if (Test-Path "$NdkHome\toolchains\llvm\prebuilt\linux-x86_64\bin") {
            $llvmBin = "$NdkHome\toolchains\llvm\prebuilt\linux-x86_64\bin"
        } elseif (Test-Path "$NdkHome\toolchains\llvm\prebuilt\darwin-x86_64\bin") {
            $llvmBin = "$NdkHome\toolchains\llvm\prebuilt\darwin-x86_64\bin"
        } else {
            Write-Tag "WARN" Yellow "Cannot locate NDK LLVM bin directory. Skipping signal-bridge."
            return
        }
    }

    # Map Rust target triples to NDK clang prefixes and jniLibs ABI dirs
    $targetInfo = @{
        "aarch64-linux-android" = @{ clang = "aarch64-linux-android24-clang"; abi = "arm64-v8a"; cargo = "AARCH64_LINUX_ANDROID" }
        "x86_64-linux-android"  = @{ clang = "x86_64-linux-android24-clang";  abi = "x86_64";    cargo = "X86_64_LINUX_ANDROID" }
    }

    foreach ($target in $Targets) {
        $info = $targetInfo[$target]
        if (-not $info) {
            Write-Tag "WARN" Yellow "Unknown Android target: $target, skipping signal-bridge for it."
            continue
        }

        # Resolve clang: prefer .cmd on Windows, bare name on Unix
        $cc = "$llvmBin\$($info.clang).cmd"
        if (-not (Test-Path $cc)) { $cc = "$llvmBin\$($info.clang)" }
        $cxx = "${cc}++"
        $ar = "$llvmBin\llvm-ar"
        if (Test-Path "$llvmBin\llvm-ar.exe") { $ar = "$llvmBin\llvm-ar.exe" }

        $cargoKey = $info.cargo
        $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER    = $null
        $env:CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER     = $null
        Set-Item "env:CARGO_TARGET_${cargoKey}_LINKER" $cc
        Set-Item "env:CC_$($target -replace '-','_')" $cc
        Set-Item "env:CXX_$($target -replace '-','_')" $cxx
        Set-Item "env:AR_$($target -replace '-','_')" $ar

        Write-Tag "BRDG" Yellow "Building signal-bridge for $target ..."
        # cargo writes compile progress to stderr; prevent $ErrorActionPreference = "Stop"
        # from treating those lines as terminating errors.
        $prevEAP = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        $buildResult = & cargo build --target $target 2>&1
        $buildExitCode = $LASTEXITCODE
        $ErrorActionPreference = $prevEAP
        if ($buildExitCode -ne 0) {
            Write-Tag "WARN" Yellow "signal-bridge build failed for $target. Persistent chat will be unavailable."
            $buildResult | ForEach-Object { Write-Tag "BRDG" Yellow $_ }
            continue
        }

        # Copy .so into jniLibs so Gradle includes it in the APK
        $soPath = Join-Path $bridgeDir "target\$target\debug\libsignal_bridge.so"
        if (-not (Test-Path $soPath)) {
            Write-Tag "WARN" Yellow "libsignal_bridge.so not found at $soPath after build."
            continue
        }

        $jniDir = Join-Path $tauriDir "gen\android\app\src\main\jniLibs\$($info.abi)"
        if (-not (Test-Path $jniDir)) { $null = New-Item -ItemType Directory -Path $jniDir -Force }
        Copy-Item -Path $soPath -Destination "$jniDir\libsignal_bridge.so" -Force
        Write-Tag "BRDG" Yellow "Placed libsignal_bridge.so in jniLibs/$($info.abi)"
    }
}

# -------------------------------------------------------------------
# Validate tools
# -------------------------------------------------------------------
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Tag "ERR" Red "cargo not found. Is Rust installed?"
    exit 1
}
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    Write-Tag "ERR" Red "npm not found. Is Node.js installed?"
    exit 1
}
if (-not $DesktopOnly) {
    $targets = rustup target list --installed 2>$null
    if ($targets -notcontains "aarch64-linux-android" -and
        $targets -notcontains "x86_64-linux-android") {
        Write-Tag "WARN" Yellow "No Android Rust targets installed. 'cargo tauri android dev' may fail."
    }
}

# -------------------------------------------------------------------
# Tracked resources for cleanup
# -------------------------------------------------------------------
$script:jobs      = @()
$script:processes = @()
$script:cfgFile   = $null

function Stop-All {
    Write-Host ""
    Write-Tag "INFO" Cyan "Shutting down..."

    foreach ($p in $script:processes) {
        if (-not $p.HasExited) {
            Write-Tag "INFO" Cyan "Stopping process $($p.Id)..."
            Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $script:processes = @()

    foreach ($j in $script:jobs) {
        if ($j.State -eq "Running") {
            Stop-Job $j -ErrorAction SilentlyContinue
        }
        Remove-Job $j -Force -ErrorAction SilentlyContinue
    }
    $script:jobs = @()

    if ($script:cfgFile -and (Test-Path $script:cfgFile)) {
        Remove-Item $script:cfgFile -ErrorAction SilentlyContinue
    }
}

$null = Register-EngineEvent PowerShell.Exiting -Action { Stop-All } -ErrorAction SilentlyContinue

try {
    # ---------------------------------------------------------------
    # Config override: skip beforeDevCommand since we run Vite ourselves.
    # Written to a temp file to avoid PowerShell quote-stripping issues.
    # ---------------------------------------------------------------
    $script:cfgFile = Join-Path ([System.IO.Path]::GetTempPath()) "tauri-dual-dev-cfg.json"
    [System.IO.File]::WriteAllText($script:cfgFile, '{"build":{"beforeDevCommand":""}}', [System.Text.UTF8Encoding]::new($false))

    # ---------------------------------------------------------------
    # 1. Start Vite dev server (background job - no GUI needed)
    # ---------------------------------------------------------------
    Write-Tag "VITE" Green "Starting Vite dev server..."
    $viteJob = Start-Job -Name "vite" -ScriptBlock {
        param($dir)
        Set-Location $dir
        & npm run dev 2>&1
    } -ArgumentList $uiDir
    $script:jobs += $viteJob

    Write-Tag "VITE" Green "Waiting for Vite on http://localhost:1420 ..."
    $ready = $false
    for ($i = 0; $i -lt 30; $i++) {
        Start-Sleep -Seconds 1
        try {
            $null = Invoke-WebRequest -Uri "http://localhost:1420" -TimeoutSec 2 -UseBasicParsing -ErrorAction Stop
            $ready = $true
            break
        } catch { }
    }
    if (-not $ready) {
        Write-Tag "ERR" Red "Vite did not start within 30 seconds."
        Receive-Job $viteJob -ErrorAction SilentlyContinue | Write-Host
        Stop-All
        exit 1
    }
    Write-Tag "VITE" Green "Vite is ready."

    # ---------------------------------------------------------------
    # 2. Desktop build (Start-Process - needs interactive session for
    #    WebView2 GUI window; runs in its own console window)
    # ---------------------------------------------------------------
    if (-not $AndroidOnly) {
        Write-Tag "DESK" Cyan "Starting desktop dev build (separate window)..."
        $desktopProc = Start-Process -FilePath "cargo" `
            -ArgumentList "tauri", "dev", "--config", $script:cfgFile `
            -WorkingDirectory $tauriDir `
            -PassThru
        $script:processes += $desktopProc
    }

    # ---------------------------------------------------------------
    # 3. Build signal-bridge for Android (if crate exists)
    # ---------------------------------------------------------------
    if (-not $DesktopOnly -and (Test-Path (Join-Path $bridgeDir "Cargo.toml"))) {
        $ndkHome = Resolve-NdkHome
        if ($ndkHome) {
            $env:NDK_HOME = $ndkHome
            if (-not $env:ANDROID_NDK_HOME) { $env:ANDROID_NDK_HOME = $ndkHome }

            # Build for all installed Android targets
            $installedTargets = rustup target list --installed 2>$null
            $androidTargets = @($installedTargets | Where-Object {
                $_ -eq "aarch64-linux-android" -or $_ -eq "x86_64-linux-android"
            })
            if ($androidTargets.Count -gt 0) {
                Push-Location $bridgeDir
                try {
                    Build-SignalBridgeAndroid -NdkHome $ndkHome -Targets $androidTargets
                } finally {
                    Pop-Location
                }
            } else {
                Write-Tag "WARN" Yellow "No Android Rust targets installed. Skipping signal-bridge."
            }
        } else {
            Write-Tag "WARN" Yellow "NDK not found. Skipping signal-bridge for Android (persistent chat will be unavailable)."
        }
    }

    # ---------------------------------------------------------------
    # 4. Android build (separate window - Tauri has its own interactive
    #    device picker that requires a real console session)
    # ---------------------------------------------------------------
    if (-not $DesktopOnly) {
        Write-Tag "ANDR" Magenta "Starting Android dev build (separate window)..."
        $androidProc = Start-Process -FilePath "cargo" `
            -ArgumentList "tauri", "android", "dev", "--config", $script:cfgFile `
            -WorkingDirectory $tauriDir `
            -PassThru
        $script:processes += $androidProc
    }

    # ---------------------------------------------------------------
    # 5. Stream background output until Ctrl+C or all done
    # ---------------------------------------------------------------
    Write-Host ""
    Write-Tag "INFO" Cyan "All processes started. Press Ctrl+C to stop."
    Write-Tag "INFO" Cyan "Desktop and Android builds are running in separate windows."
    Write-Tag "INFO" Cyan "Pick your Android device in the Android window when prompted."
    Write-Host ""

    $tagMap = @{
        "vite"    = @{ Tag = "VITE"; Color = [ConsoleColor]::Green }
        "android" = @{ Tag = "ANDR"; Color = [ConsoleColor]::Magenta }
    }

    while ($true) {
        $anyRunning = $false

        # Check background jobs
        foreach ($j in $script:jobs) {
            if ($j.State -eq "Running") { $anyRunning = $true }
            $output = Receive-Job $j -ErrorAction SilentlyContinue
            if ($output) {
                $info = $tagMap[$j.Name]
                foreach ($line in $output) {
                    $text = if ($line -is [System.Management.Automation.ErrorRecord]) {
                        $line.ToString()
                    } else { "$line" }
                    Write-Host "[$($info.Tag)] " -ForegroundColor $info.Color -NoNewline
                    Write-Host $text
                }
            }
        }

        # Check desktop process
        foreach ($p in $script:processes) {
            if (-not $p.HasExited) { $anyRunning = $true }
        }

        if (-not $anyRunning) {
            Write-Tag "INFO" Yellow "All processes have exited."
            break
        }
        Start-Sleep -Milliseconds 250
    }
}
finally {
    Stop-All
}
