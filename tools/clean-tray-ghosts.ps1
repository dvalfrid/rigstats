# clean-tray-ghosts.ps1
# Removes ghost RIGStats entries from Windows system tray icon settings.
# Run as the same user that runs RIGStats (no elevation needed).

$base = "HKCU:\Control Panel\NotifyIconSettings"

# Find all tray entries that mention rigstats in any form
$all = Get-ChildItem $base -ErrorAction SilentlyContinue
$rigstats = $all | Where-Object {
    $exe = (Get-ItemProperty $_.PSPath -Name "ExecutablePath" -ErrorAction SilentlyContinue).ExecutablePath
    $exe -match "rigstats|RigStats|RIGStats|LibreHardware"
}

if (-not $rigstats) {
    Write-Host "No RIGStats entries found."
    exit 0
}

# Classify each entry: KEEP = current install, DELETE = ghost
$keep   = @()
$delete = @()

foreach ($entry in $rigstats) {
    $exe = (Get-ItemProperty $entry.PSPath -Name "ExecutablePath" -ErrorAction SilentlyContinue).ExecutablePath
    $tip = (Get-ItemProperty $entry.PSPath -Name "InitialTooltip" -ErrorAction SilentlyContinue).InitialTooltip

    # Current valid install: Program Files\RIGStats\rigstats.exe (exact casing, no \target\)
    $isValid = ($exe -match "RIGStats\\rigstats\.exe$") -and ($exe -notmatch "\\target\\")

    if ($isValid) {
        $keep += [PSCustomObject]@{ Id = $entry.PSChildName; Exe = $exe; Tip = $tip }
    } else {
        $delete += [PSCustomObject]@{ Id = $entry.PSChildName; Exe = $exe; Tip = $tip }
    }
}

Write-Host ""
Write-Host "=== KEEP ===" -ForegroundColor Green
$keep | ForEach-Object { Write-Host "  $($_.Id): $($_.Exe)" }

Write-Host ""
Write-Host "=== DELETE ===" -ForegroundColor Yellow
$delete | ForEach-Object { Write-Host "  $($_.Id): $($_.Exe)" }

if (-not $delete) {
    Write-Host ""
    Write-Host "Nothing to delete." -ForegroundColor Green
    exit 0
}

Write-Host ""
$confirm = Read-Host "Continue? (y/n)"
if ($confirm -ne "y") {
    Write-Host "Aborted."
    exit 0
}

Write-Host ""
Write-Host "Stopping Explorer..."
Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 1500

foreach ($entry in $delete) {
    $path = "$base\$($entry.Id)"
    if (Test-Path $path) {
        Remove-Item $path -Recurse -Force
        Write-Host "Removed: $($entry.Id) ($($entry.Exe))" -ForegroundColor Green
    }
}

Start-Process explorer
Write-Host ""
Write-Host "Done. Check Settings -> Personalization -> Taskbar -> Other system tray icons." -ForegroundColor Cyan
