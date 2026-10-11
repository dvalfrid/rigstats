# Download counts per release, from GitHub's release assets:
#
#   installer — downloads of RIGStats_*_x64-setup.exe: new installs from the
#               website/GitHub, winget installs and upgrades (winget fetches
#               the file from GitHub), and the app's own updates.
#   checks    — downloads of latest.json: the app's update checks (about 10 s
#               after start, then every 6 h). It fetches
#               releases/latest/download/latest.json, so a check counts on the
#               release that was newest at the time — the best measure of
#               active installations.
#
# GitHub keeps only running totals (no history, no unique users), so the
# weekly workflow (.github/workflows/download-stats.yml) appends a snapshot to
# downloads.csv on the `stats` branch.
#
#   pwsh -File tools\download-stats.ps1              # table of recent releases
#   pwsh -File tools\download-stats.ps1 -Top 0       # every release
#   pwsh -File tools\download-stats.ps1 -Append x.csv  # add a snapshot to a CSV
#
# Needs the GitHub CLI (gh), logged in, or GH_TOKEN set (as in the workflow).
param(
    [string]$Repo = 'dvalfrid/rigstats',
    [int]$Top = 12,
    [string]$Append
)

$ErrorActionPreference = 'Stop'

$jq = '.[] | {tag: .tag_name, published: .published_at, ' +
      'installer: ([.assets[] | select(.name | endswith(".exe")) | .download_count] | add // 0), ' +
      'checks: ([.assets[] | select(.name == "latest.json") | .download_count] | add // 0)}'
$lines = gh api --paginate "repos/$Repo/releases?per_page=100" --jq $jq
if ($LASTEXITCODE -ne 0) { throw "gh api failed ($LASTEXITCODE)" }
$releases = @($lines | ForEach-Object { $_ | ConvertFrom-Json } |
    Where-Object { $_.published } |
    Sort-Object { [datetime]$_.published } -Descending)

$today = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd')

if ($Append) {
    $new = -not (Test-Path $Append)
    $rows = $releases | ForEach-Object {
        '{0},{1},{2},{3},{4}' -f $today, $_.tag, ([datetime]$_.published).ToString('yyyy-MM-dd'), $_.installer, $_.checks
    }
    if ($new) { 'snapshot,tag,published,installer,checks' | Set-Content $Append -Encoding utf8NoBOM }
    $rows | Add-Content $Append -Encoding utf8NoBOM
    Write-Host "Appended $($rows.Count) releases to $Append (snapshot $today)"
    return
}

# Per day while each release was the newest, so short- and long-lived
# releases compare.
$now = (Get-Date).ToUniversalTime()
$shown = if ($Top -gt 0) { [Math]::Min($Top, $releases.Count) } else { $releases.Count }
$table = for ($i = 0; $i -lt $shown; $i++) {
    $r = $releases[$i]
    $start = [datetime]$r.published
    $end = if ($i -eq 0) { $now } else { [datetime]$releases[$i - 1].published }
    $days = [Math]::Max(1, ($end - $start).TotalDays)
    [pscustomobject]@{
        Release         = $r.tag
        Published       = $start.ToString('yyyy-MM-dd')
        'Days newest'   = [Math]::Round($days, 1)
        Installer       = $r.installer
        'Installer/day' = [int][Math]::Round($r.installer / $days)
        Checks          = $r.checks
        'Checks/day'    = [int][Math]::Round($r.checks / $days)
    }
}
$table | Format-Table -AutoSize
$totalInstaller = ($releases | Measure-Object installer -Sum).Sum
$totalChecks = ($releases | Measure-Object checks -Sum).Sum
"{0} releases — installer downloads: {1}, update checks: {2}" -f $releases.Count, $totalInstaller, $totalChecks
