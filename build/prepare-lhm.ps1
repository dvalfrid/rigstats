param(
    [string]$Version = '0.9.6',
    [switch]$ForceDownload
)

$ErrorActionPreference = 'Stop'

$projectRoot = Split-Path -Parent $PSScriptRoot
$lhmDir = Join-Path $projectRoot 'vendor\lhm'
$lhmExe = Join-Path $lhmDir 'LibreHardwareMonitor.exe'
$versionMarker = Join-Path $lhmDir '.rigstats-version'
$downloadUrl = "https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases/download/v$Version/LibreHardwareMonitor.zip"

$installedVersion = $null
if (Test-Path $lhmExe) {
    $fileVersion = (Get-Item $lhmExe).VersionInfo.FileVersion
    if ($fileVersion -match '^(\d+\.\d+\.\d+)') {
        $installedVersion = $Matches[1]
    }
}

if ((Test-Path $lhmExe) -and (Test-Path $versionMarker) -and -not $ForceDownload) {
    $currentVersion = (Get-Content -Path $versionMarker -Raw).Trim()
    if ($currentVersion -eq $Version) {
    Write-Host "LibreHardwareMonitor already prepared at $lhmDir"
    exit 0
    }
}

if ((Test-Path $lhmExe) -and ($installedVersion -eq $Version) -and -not $ForceDownload) {
    Set-Content -Path $versionMarker -Value $Version -NoNewline
    Write-Host "LibreHardwareMonitor already matches v$Version at $lhmDir"
    exit 0
}

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("rigstats-lhm-" + [System.Guid]::NewGuid().ToString('N'))
$archivePath = Join-Path $tempDir 'LibreHardwareMonitor.zip'

try {
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
    New-Item -ItemType Directory -Path $lhmDir -Force | Out-Null

    Write-Host "Downloading LibreHardwareMonitor v$Version"
    Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath

    try {
        Get-ChildItem -Path $lhmDir -Recurse -Force -File -ErrorAction SilentlyContinue |
            ForEach-Object { $_.IsReadOnly = $false }
        Get-ChildItem -Path $lhmDir -Force -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force
    }
    catch {
        throw "Failed to refresh $lhmDir. Stop any running process that is using files from vendor/lhm and try again. Original error: $($_.Exception.Message)"
    }

    Expand-Archive -LiteralPath $archivePath -DestinationPath $lhmDir -Force

    if (-not (Test-Path $lhmExe)) {
        throw "LibreHardwareMonitor.exe was not found after extracting $downloadUrl"
    }

    Set-Content -Path $versionMarker -Value $Version -NoNewline

    Write-Host "Prepared LibreHardwareMonitor v$Version in $lhmDir"
}
finally {
    if (Test-Path $tempDir) {
        Remove-Item -Path $tempDir -Recurse -Force
    }
}