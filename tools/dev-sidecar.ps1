# Runs the debug sensor sidecar in place of the installed rigstats-sensor
# service, for live-testing Control Center features (fans, CPU/GPU limits,
# Curve Optimizer, lighting). Run in an ELEVATED PowerShell:
#   pwsh -File tools\dev-sidecar.ps1         -> dry-run: UI works, no hardware writes
#   pwsh -File tools\dev-sidecar.ps1 -Live   -> real writes to the hardware
# Stop with Ctrl+C: the dev sidecar hands everything back to the firmware, then
# the installed rigstats-sensor service is started again. While it runs the
# sidecar exe is locked — stop it before `dotnet build sensor-sidecar`.
# See "Live testing on real hardware" in docs/control-architecture.md.
param([switch]$Live)

$exe = Join-Path $PSScriptRoot '..\sensor-sidecar\bin\Debug\net10.0-windows\win-x64\rigstats-sensor.exe'
if (-not (Test-Path $exe)) {
    Write-Host "Build the sidecar first: dotnet build sensor-sidecar/sensor-sidecar.csproj" -ForegroundColor Red
    exit 1
}

sc.exe stop rigstats-sensor | Out-Null
Start-Sleep -Seconds 2
try {
    if ($Live) {
        Write-Host 'LIVE mode - changes are written to the hardware.' -ForegroundColor Yellow
        & $exe
    } else {
        Write-Host 'DRY-RUN mode - no hardware writes.' -ForegroundColor Green
        & $exe --dry-run
    }
} finally {
    sc.exe start rigstats-sensor | Out-Null
    Write-Host 'Installed rigstats-sensor service started again.'
}
