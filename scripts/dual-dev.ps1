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

# -------------------------------------------------------------------
# Helpers
# -------------------------------------------------------------------
function Write-Tag {
    param([string]$Tag, [ConsoleColor]$Color, [string]$Message)
    Write-Host "[$Tag] " -ForegroundColor $Color -NoNewline
    Write-Host $Message
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
    # 3. Android build (separate window - Tauri has its own interactive
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
    # 4. Stream background output until Ctrl+C or all done
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
