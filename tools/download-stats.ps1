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
#   pwsh -File tools\download-stats.ps1 -ByMinor     # summed per minor version (1.49.x)
#   pwsh -File tools\download-stats.ps1 -Trend       # update checks per day, week by week
#   pwsh -File tools\download-stats.ps1 -Append x.csv  # add a snapshot to a CSV
#
# -Trend spreads each release's checks over the days it was the newest and
# sums them per ISO week: checks per day follow the installations that run.
# "Active" turns that into a range — an installation checks about twice a
# day when its PC is on for a working day (start + 6 h) and about five times
# when it runs around the clock — so it is an estimate, and the owner's own
# PCs are in it. Release candidates are left out: they never become
# releases/latest, and their checks go to their own latest.json.
#
# Needs the GitHub CLI (gh), logged in, or GH_TOKEN set (as in the workflow).
param(
    [string]$Repo = 'dvalfrid/rigstats',
    [int]$Top = 12,
    [string]$Append,
    [switch]$ByMinor,
    [switch]$Trend
)

$ErrorActionPreference = 'Stop'

$jq = '.[] | {tag: .tag_name, published: .published_at, prerelease: .prerelease, ' +
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

$now = (Get-Date).ToUniversalTime()
# Only real releases were ever releases/latest.
$stable = @($releases | Where-Object { -not $_.prerelease })

if ($ByMinor) {
    $releases | Group-Object {
        $v = $_.tag -replace '^v', '' -replace '-.*$', ''
        $minor = ($v -split '\.')[0..1] -join '.'
        if ($_.prerelease) { "$minor (RC)" } else { $minor }
    } | ForEach-Object {
        $first = $_.Group | Sort-Object { [datetime]$_.published } | Select-Object -First 1
        [pscustomobject]@{
            Version   = $_.Name
            Releases  = $_.Count
            Installer = [int]($_.Group | Measure-Object installer -Sum).Sum
            Checks    = [int]($_.Group | Measure-Object checks -Sum).Sum
            First     = ([datetime]$first.published).ToString('yyyy-MM-dd')
        }
    } | Sort-Object { [datetime]$_.First } -Descending | Format-Table -AutoSize
    return
}

if ($Trend) {
    # Checks per day while each release was the newest, laid out day by day
    # and summed per ISO week.
    $weeks = [ordered]@{}
    $ascending = @($stable | Sort-Object { [datetime]$_.published })
    for ($i = 0; $i -lt $ascending.Count; $i++) {
        $start = ([datetime]$ascending[$i].published).ToUniversalTime()
        $end = if ($i + 1 -lt $ascending.Count) { ([datetime]$ascending[$i + 1].published).ToUniversalTime() } else { $now }
        $days = ($end - $start).TotalDays
        if ($days -le 0 -or $ascending[$i].checks -eq 0) { continue }
        $rate = $ascending[$i].checks / $days
        $at = $start
        while ($at -lt $end) {
            $next = [datetime]::new($at.Year, $at.Month, $at.Day, 0, 0, 0, [DateTimeKind]::Utc).AddDays(1)
            if ($next -gt $end) { $next = $end }
            $key = '{0}-W{1:D2}' -f [System.Globalization.ISOWeek]::GetYear($at), [System.Globalization.ISOWeek]::GetWeekOfYear($at)
            if (-not $weeks.Contains($key)) { $weeks[$key] = 0.0 }
            $weeks[$key] += $rate * ($next - $at).TotalDays
            $at = $next
        }
    }
    $thisWeek = '{0}-W{1:D2}' -f [System.Globalization.ISOWeek]::GetYear($now), [System.Globalization.ISOWeek]::GetWeekOfYear($now)
    $previous = $null
    $rows = foreach ($key in ($weeks.Keys | Sort-Object)) {
        $year, $week = $key -split '-W'
        $monday = [System.Globalization.ISOWeek]::ToDateTime([int]$year, [int]$week, [DayOfWeek]::Monday)
        # The running week counts only its days so far.
        $span = if ($key -eq $thisWeek) { [Math]::Max(1 / 24, ($now - $monday).TotalDays) } else { 7 }
        $perDay = $weeks[$key] / $span
        [pscustomobject]@{
            Week         = $key + $(if ($key -eq $thisWeek) { ' (so far)' } else { '' })
            From         = $monday.ToString('yyyy-MM-dd')
            'Checks/day' = [int][Math]::Round($perDay)
            Change       = if ($previous) { '{0:+0;-0;0} %' -f (100 * ($perDay - $previous) / $previous) } else { '' }
            Active       = '{0}–{1}' -f [int][Math]::Round($perDay / 5), [int][Math]::Round($perDay / 2)
        }
        $previous = $perDay
    }
    $rows | Format-Table -AutoSize
    'Active: estimated installations (2–5 checks a day each), the owner''s own PCs included.'
    return
}

# Per day while each release was the newest, so short- and long-lived
# releases compare.
$shown = if ($Top -gt 0) { [Math]::Min($Top, $stable.Count) } else { $stable.Count }
$table = for ($i = 0; $i -lt $shown; $i++) {
    $r = $stable[$i]
    $start = [datetime]$r.published
    $end = if ($i -eq 0) { $now } else { [datetime]$stable[$i - 1].published }
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
$totalInstaller = ($stable | Measure-Object installer -Sum).Sum
$totalChecks = ($stable | Measure-Object checks -Sum).Sum
"{0} releases — installer downloads: {1}, update checks: {2} (release candidates not counted)" -f $stable.Count, $totalInstaller, $totalChecks
