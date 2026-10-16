# Runs the debug sensor sidecar in place of the installed rigstats-sensor
# service, for live-testing Control Center features (fans, CPU/GPU limits,
# Curve Optimizer, lighting). Run in an ELEVATED PowerShell:
#   pwsh -File tools\dev-sidecar.ps1         -> dry-run: UI works, no hardware writes
#   pwsh -File tools\dev-sidecar.ps1 -Live   -> real writes to the hardware
#   ... -SidecarArgs --gpu-worker-crash-test -> extra sidecar arguments (debug
#       builds): --gpu-worker-crash-test / --gpu-worker-hang-test (#370),
#       --hardware-worker-crash-after <s> / --hardware-worker-hang-after <s> /
#       --hardware-worker-crash-reading <hardware id prefix> (#374).
#       An argument with a value is a list — run the script from a PowerShell
#       prompt, not through `pwsh -File` (which passes 'a','b' as one string):
#       .\tools\dev-sidecar.ps1 -Live -SidecarArgs '--hardware-worker-crash-after','30'
# Use the DEBUG app with it: the release app only talks to the real service.
# Stop with Ctrl+C: the dev sidecar hands everything back to the firmware, then
# the installed rigstats-sensor service is started again. While it runs the
# sidecar exe is locked — stop it before `dotnet build sensor-sidecar`.
# See "Live testing on real hardware" in docs/control-architecture.md.
param([switch]$Live, [string[]]$SidecarArgs = @())

$exe = Join-Path $PSScriptRoot '..\sensor-sidecar\bin\Debug\net10.0-windows\win-x64\rigstats-sensor.exe'
if (-not (Test-Path $exe)) {
    Write-Host "Build the sidecar first: dotnet build sensor-sidecar/sensor-sidecar.csproj" -ForegroundColor Red
    exit 1
}

# Clear the service's recovery actions first: a service that crashes would
# otherwise be restarted by Windows and run beside the dev sidecar (#370).
# They are put back in `finally` (same values as build/installer.nsi).
sc.exe failure rigstats-sensor reset= 0 actions= "" | Out-Null
# A service still starting can't take a stop, so ask again until it has
# stopped; after ~15 s end its process (by the service's own PID).
for ($i = 0; $i -lt 30; $i++) {
    $service = Get-CimInstance Win32_Service -Filter "Name='rigstats-sensor'"
    if (-not $service -or $service.State -eq 'Stopped') { break }
    sc.exe stop rigstats-sensor | Out-Null
    Start-Sleep -Milliseconds 500
}
$service = Get-CimInstance Win32_Service -Filter "Name='rigstats-sensor'"
if ($service -and $service.ProcessId) {
    Write-Host "rigstats-sensor did not stop - ending its process ($($service.ProcessId))." -ForegroundColor Yellow
    Stop-Process -Id $service.ProcessId -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
}
try {
    if ($Live) {
        Write-Host 'LIVE mode - changes are written to the hardware.' -ForegroundColor Yellow
        & $exe @SidecarArgs
    } else {
        Write-Host 'DRY-RUN mode - no hardware writes.' -ForegroundColor Green
        & $exe --dry-run @SidecarArgs
    }
} finally {
    sc.exe failure rigstats-sensor reset= 60 actions= restart/5000/restart/10000/restart/30000 | Out-Null
    sc.exe start rigstats-sensor | Out-Null
    Write-Host 'Installed rigstats-sensor service started again.'
}
