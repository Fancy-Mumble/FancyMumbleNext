<#
.SYNOPSIS
    Kill any process holding the Tauri/Vite dev server port (1420).
.DESCRIPTION
    Finds and terminates the process(es) bound to TCP port 1420, which
    is the default port used by the Vite dev server inside 'cargo tauri dev'
    and 'cargo tauri android dev'. Useful when a previous dev server session
    did not exit cleanly and the port is still occupied.
.PARAMETER Port
    TCP port to free. Defaults to 1420 (Vite dev server).
.EXAMPLE
    .\kill-dev-server.ps1               # Kill whatever holds port 1420
.EXAMPLE
    .\kill-dev-server.ps1 -Port 3000    # Kill whatever holds a custom port
#>
param(
    [int]$Port = 1420
)

$ErrorActionPreference = "Continue"
if (Test-Path variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

Write-Host "`nFancy Mumble - Kill Dev Server (port $Port)" -ForegroundColor Cyan
Write-Host ("=" * 48)

$connections = Get-NetTCPConnection -LocalPort $Port -ErrorAction SilentlyContinue

if (-not $connections) {
    Write-Host "  [OK] Port $Port is not in use. Nothing to kill." -ForegroundColor Green
    exit 0
}

$owningPids = $connections.OwningProcess | Sort-Object -Unique

foreach ($ownerPid in $owningPids) {
    $proc = Get-Process -Id $ownerPid -ErrorAction SilentlyContinue
    if ($proc) {
        Write-Host "  [..] Killing PID $ownerPid ($($proc.ProcessName)) on port $Port ..." -ForegroundColor Yellow
        Stop-Process -Id $ownerPid -Force -ErrorAction SilentlyContinue
        # Confirm
        Start-Sleep -Milliseconds 400
        if (-not (Get-Process -Id $ownerPid -ErrorAction SilentlyContinue)) {
            Write-Host "  [OK] PID $ownerPid terminated." -ForegroundColor Green
        } else {
            Write-Host "  [!!] PID $ownerPid could not be terminated. Try running as Administrator." -ForegroundColor Red
        }
    } else {
        Write-Host "  [??] PID $ownerPid holds port $Port but the process no longer exists." -ForegroundColor DarkGray
    }
}

# Final check
$remaining = Get-NetTCPConnection -LocalPort $Port -ErrorAction SilentlyContinue
if ($remaining) {
    Write-Host "  [!!] Port $Port is still in use. Manual intervention may be needed." -ForegroundColor Red
    exit 1
} else {
    Write-Host "  [OK] Port $Port is now free." -ForegroundColor Green
}
