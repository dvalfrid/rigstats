# clean-tray-ghosts.ps1
# Removes ghost RIGStats entries from:
#   - Windows system tray icon settings (HKCU)
#   - Installed Apps / Programs & Features (HKLM) when the uninstaller is missing
#
# Requests elevation automatically if not already running as admin.

# ── Elevation ─────────────────────────────────────────────────────────────────
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Start-Process pwsh "-ExecutionPolicy Bypass -File `"$PSCommandPath`"" -Verb RunAs
    exit
}

$anyWork = $false

# ── Orphaned Installed Apps entries ───────────────────────────────────────────
Write-Host ""
Write-Host "=== INSTALLED APPS ===" -ForegroundColor Cyan

$uninstallRoots = @(
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
)

$orphaned = @()
foreach ($root in $uninstallRoots) {
    Get-ChildItem $root -ErrorAction SilentlyContinue | Where-Object {
        (Get-ItemProperty $_.PSPath -Name "DisplayName" -ErrorAction SilentlyContinue).DisplayName -match "RIGStats|RigStats"
    } | ForEach-Object {
        $uninstallStr = (Get-ItemProperty $_.PSPath -Name "UninstallString" -ErrorAction SilentlyContinue).UninstallString
        $exePath = $uninstallStr -replace '"', '' -replace ' /.*$', ''
        $version  = (Get-ItemProperty $_.PSPath -Name "DisplayVersion" -ErrorAction SilentlyContinue).DisplayVersion
        $isOrphaned = $uninstallStr -and (-not (Test-Path $exePath))
        if ($isOrphaned) {
            $orphaned += [PSCustomObject]@{ Key = $_.PSPath; Version = $version; Uninstaller = $exePath }
        } else {
            Write-Host "  OK: RIGStats $version — uninstaller present" -ForegroundColor Green
        }
    }
}

if ($orphaned) {
    Write-Host "  Orphaned entries (uninstaller missing):" -ForegroundColor Yellow
    $orphaned | ForEach-Object { Write-Host "    $($_.Key) (v$($_.Version))" }
    $anyWork = $true
} else {
    Write-Host "  No orphaned entries found." -ForegroundColor Green
}

# ── Ghost system tray entries ─────────────────────────────────────────────────
Write-Host ""
Write-Host "=== SYSTEM TRAY ===" -ForegroundColor Cyan

$trayBase = "HKCU:\Control Panel\NotifyIconSettings"
$all = Get-ChildItem $trayBase -ErrorAction SilentlyContinue
$rigstatsTray = $all | Where-Object {
    $exe = (Get-ItemProperty $_.PSPath -Name "ExecutablePath" -ErrorAction SilentlyContinue).ExecutablePath
    $exe -match "rigstats|RigStats|RIGStats|LibreHardware"
}

$keep   = @()
$delete = @()
foreach ($entry in $rigstatsTray) {
    $exe = (Get-ItemProperty $entry.PSPath -Name "ExecutablePath" -ErrorAction SilentlyContinue).ExecutablePath
    $isValid = ($exe -match "RIGStats\\rigstats\.exe$") -and ($exe -notmatch "\\target\\")
    if ($isValid) {
        $keep += [PSCustomObject]@{ Id = $entry.PSChildName; Exe = $exe }
    } else {
        $delete += [PSCustomObject]@{ Id = $entry.PSChildName; Exe = $exe }
    }
}

if ($keep) {
    Write-Host "  Keep:" -ForegroundColor Green
    $keep | ForEach-Object { Write-Host "    $($_.Exe)" }
}
if ($delete) {
    Write-Host "  Ghost entries to remove:" -ForegroundColor Yellow
    $delete | ForEach-Object { Write-Host "    $($_.Exe)" }
    $anyWork = $true
} else {
    Write-Host "  No ghost tray entries found." -ForegroundColor Green
}

# ── Confirm and clean ─────────────────────────────────────────────────────────
if (-not $anyWork) {
    Write-Host ""
    Write-Host "Nothing to clean up." -ForegroundColor Green
    Read-Host "`nPress Enter to exit"
    exit 0
}

Write-Host ""
$confirm = Read-Host "Remove the items listed above? (y/n)"
if ($confirm -ne "y") {
    Write-Host "Aborted."
    Read-Host "`nPress Enter to exit"
    exit 0
}

# Remove orphaned uninstall entries
foreach ($entry in $orphaned) {
    Remove-Item $entry.Key -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "Removed orphaned uninstall entry: $($entry.Key)" -ForegroundColor Green
}

# Remove ghost tray entries (requires Explorer restart)
if ($delete) {
    Write-Host "Stopping Explorer..."
    Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 1500

    foreach ($entry in $delete) {
        $path = "$trayBase\$($entry.Id)"
        if (Test-Path $path) {
            Remove-Item $path -Recurse -Force
            Write-Host "Removed tray ghost: $($entry.Exe)" -ForegroundColor Green
        }
    }

    Start-Process explorer
}

Write-Host ""
Write-Host "Done." -ForegroundColor Cyan
Write-Host "  - Installed Apps: refresh Settings -> Apps -> Installed apps"
Write-Host "  - System tray:    check Settings -> Personalization -> Taskbar -> Other system tray icons"
Read-Host "`nPress Enter to exit"
