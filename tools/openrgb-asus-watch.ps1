# Checks OpenRGB's ASUS device lists for entries added since the last review.
# OpenRGB is read as protocol documentation only (product ids and names);
# no code is copied. Run weekly (see tools/README.md):
#   pwsh -NoProfile -File tools/openrgb-asus-watch.ps1                  -> report new entries
#   pwsh -NoProfile -File tools/openrgb-asus-watch.ps1 -Json            -> the same, as JSON
#   pwsh -NoProfile -File tools/openrgb-asus-watch.ps1 -UpdateBaseline  -> mark everything as reviewed
# Each new entry says whether RIGStats already has the driver for its protocol
# family (then adding it is mostly a table entry plus a hardware check) or not.
param([switch]$Json, [switch]$UpdateBaseline)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$baselinePath = Join-Path $PSScriptRoot 'openrgb-asus-baseline.json'
$rawBase = 'https://gitlab.com/CalcProgrammer1/OpenRGB/-/raw/master/'
$treeApi = 'https://gitlab.com/api/v4/projects/CalcProgrammer1%2FOpenRGB/repository/tree'
# OpenRGB's USB ASUS controller folders. The files inside are listed from
# GitLab rather than named here: OpenRGB moves and splits them (#722).
$folders = @(
    'Controllers/AsusAuraUSBController'
    'Controllers/AsusMonitorController'
    'Controllers/AsusAuraCoreController'
    'Controllers/AsusLegacyUSBController'
)
$paths = foreach ($folder in $folders) {
    for ($page = 1; ; $page++) {
        # Assigned first: @(Invoke-RestMethod ...) wraps the whole JSON array as one item.
        $items = Invoke-RestMethod "${treeApi}?path=$folder&recursive=true&per_page=100&page=$page"
        $items = @($items)
        $items | Where-Object type -eq 'blob' | ForEach-Object path
        if ($items.Count -lt 100) { break }
    }
}
$files = @($paths | Where-Object { $_ -like '*Detect.cpp' })
# Headers that define product ids the detectors use (mouse ids live here).
$headers = @($paths | Where-Object { $_ -like '*Devices.h' })
if ($files.Count -eq 0) { throw "No *Detect.cpp found under $($folders -join ', ') - did OpenRGB move them?" }

# OpenRGB detector -> the RIGStats driver for that protocol family, if any.
$drivers = @{
    'DetectAsusAuraUSBKeyboard'     = 'AsusKeyboardDevice.Model (keyboard table)'
    'DetectAsusMonitorControllers'  = 'AsusMonitorDevice.Model (monitor table)'
    'DetectAsusAuraUSBMotherboards' = 'AuraUsb.Family (Aura USB controller)'
    'DetectAsusAuraUSBAddressable'  = 'AuraUsb.Family (Aura USB controller)'
}

# Every product id RIGStats' lighting code mentions.
$known = @{}
Get-ChildItem (Join-Path $root 'sensor-sidecar/Control/Lighting') -Filter *.cs | ForEach-Object {
    foreach ($m in [regex]::Matches((Get-Content $_.FullName -Raw), '0x([0-9A-Fa-f]{4})\b')) {
        $known[$m.Groups[1].Value.ToUpperInvariant()] = $true
    }
}

function Get-Source([string]$path) { (Invoke-WebRequest -Uri ($rawBase + $path) -UseBasicParsing).Content }
$defines = @{}
function Add-Defines([string]$text) {
    foreach ($m in [regex]::Matches($text, '#define\s+(\w+)\s+0x([0-9A-Fa-f]{4})\b')) {
        $defines[$m.Groups[1].Value] = $m.Groups[2].Value.ToUpperInvariant()
    }
}
foreach ($header in $headers) { Add-Defines (Get-Source $header) }

$entries = foreach ($file in $files) {
    $text = Get-Source $file
    Add-Defines $text
    $register = 'REGISTER_HID_DETECTOR\w*\s*\(\s*"([^"]+)"\s*,\s*(\w+)\s*,\s*\w+\s*,\s*(\w+)'
    foreach ($m in [regex]::Matches($text, $register)) {
        $token = $m.Groups[3].Value
        $productId = if ($token -match '^0x([0-9A-Fa-f]{4})$') { $Matches[1].ToUpperInvariant() } else { $defines[$token] }
        if (-not $productId) { Write-Warning "No product id for $token in $file"; continue }
        [pscustomobject]@{ Pid = $productId; Name = $m.Groups[1].Value; Detector = $m.Groups[2].Value }
    }
}
$entries = $entries | Sort-Object Pid, Name -Unique

if ($UpdateBaseline) {
    $entries | Select-Object Pid, Name, Detector | ConvertTo-Json -Depth 3 | Set-Content $baselinePath -Encoding utf8
    "Baseline updated: $($entries.Count) OpenRGB ASUS entries marked as reviewed."
    return
}

$baseline = @{}
if (Test-Path $baselinePath) {
    foreach ($b in (Get-Content $baselinePath -Raw | ConvertFrom-Json)) { $baseline["$($b.Pid) $($b.Name)"] = $true }
}
$new = @($entries | Where-Object { -not $baseline["$($_.Pid) $($_.Name)"] } | ForEach-Object {
    [pscustomobject]@{
        Pid           = $_.Pid
        Name          = $_.Name
        Detector      = $_.Detector
        RigstatsDriver = $drivers[$_.Detector]
        KnownToRigstats = [bool]$known[$_.Pid]
    }
})

if ($Json) {
    ConvertTo-Json -InputObject $new -Depth 3
    return
}
if ($new.Count -eq 0) {
    "No new ASUS entries in OpenRGB since the last review ($($entries.Count) checked)."
    return
}
"New ASUS entries in OpenRGB since the last review:"
$new | Format-Table Pid, Name, Detector, RigstatsDriver, KnownToRigstats -AutoSize | Out-String -Width 220
