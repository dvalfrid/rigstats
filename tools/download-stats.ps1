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
# downloads.csv on the `stats` branch and writes a weekly report next to it.
#
#   pwsh -File tools\download-stats.ps1              # table of recent releases
#   pwsh -File tools\download-stats.ps1 -Top 0       # every release
#   pwsh -File tools\download-stats.ps1 -ByMinor     # summed per minor version (1.49.x)
#   pwsh -File tools\download-stats.ps1 -Trend       # update checks per day, week by week
#   pwsh -File tools\download-stats.ps1 -Append x.csv  # add a snapshot to a CSV
#   pwsh -File tools\download-stats.ps1 -Report reports -Csv x.csv
#                                                    # the weekly report in Markdown
#
# -Trend spreads each release's checks over the days it was the newest and
# sums them per ISO week: checks per day follow the installations that run.
# "Active" turns that into a range — an installation checks about twice a
# day when its PC is on for a working day (start + 6 h) and about five times
# when it runs around the clock — so it is an estimate, and the owner's own
# PCs are in it. Release candidates are left out: they never become
# releases/latest, and their checks go to their own latest.json.
#
# -Report writes one Markdown page into the folder it names, as
# <year>-W<week>.md for the last complete ISO week and again as latest.md:
# the week in numbers, the downloads per release since the CSV's previous
# snapshot (when -Csv is given), the trend and the totals per minor version.
#
# Needs the GitHub CLI (gh), logged in, or GH_TOKEN set (as in the workflow).
param(
    [string]$Repo = 'dvalfrid/rigstats',
    [int]$Top = 12,
    [string]$Append,
    [switch]$ByMinor,
    [switch]$Trend,
    [string]$Report,
    [string]$Csv
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

$now = (Get-Date).ToUniversalTime()
$today = $now.ToString('yyyy-MM-dd')
# Only real releases were ever releases/latest.
$stable = @($releases | Where-Object { -not $_.prerelease })

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

function Get-ByMinor {
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
    } | Sort-Object { [datetime]$_.First } -Descending
}

# Checks per day while each release was the newest, laid out day by day and
# summed per ISO week; the running week counts only its days so far.
function Get-Trend {
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
    foreach ($key in ($weeks.Keys | Sort-Object)) {
        $year, $week = $key -split '-W'
        $monday = [System.Globalization.ISOWeek]::ToDateTime([int]$year, [int]$week, [DayOfWeek]::Monday)
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
}

# Per day while each release was the newest, so short- and long-lived
# releases compare.
function Get-PerRelease([int]$count) {
    $shown = if ($count -gt 0) { [Math]::Min($count, $stable.Count) } else { $stable.Count }
    for ($i = 0; $i -lt $shown; $i++) {
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
}

# A Markdown table from objects, columns in their property order.
function ConvertTo-MarkdownTable($rows) {
    $rows = @($rows)
    if ($rows.Count -eq 0) { return '_None._' }
    $names = @($rows[0].PSObject.Properties.Name)
    # Each line in its own parentheses: the comma binds tighter than +.
    $out = @(('| ' + ($names -join ' | ') + ' |'), ('|' + (($names | ForEach-Object { '---' }) -join '|') + '|'))
    foreach ($row in $rows) {
        $out += '| ' + (($names | ForEach-Object { "$($row.$_)" }) -join ' | ') + ' |'
    }
    $out -join "`n"
}

if ($ByMinor) {
    Get-ByMinor | Format-Table -AutoSize
    return
}

if ($Trend) {
    Get-Trend | Format-Table -AutoSize
    'Active: estimated installations (2–5 checks a day each), the owner''s own PCs included.'
    return
}

if ($Report) {
    $weekly = @(Get-Trend)
    # The last complete week: on a Monday morning the new one has hours.
    $complete = @($weekly | Where-Object { $_.Week -notmatch 'so far' })
    if ($complete.Count -eq 0) { $complete = $weekly }
    $current = $complete[-1]
    $last = if ($complete.Count -gt 1) { $complete[-2] } else { $null }
    # The report's trend: the last 12 weeks, newest first.
    $recent = @($weekly | Select-Object -Last 12)
    [array]::Reverse($recent)
    $week = $current.Week -replace ' \(so far\)', ''

    # This week's downloads: today's totals minus the CSV's previous snapshot.
    $sinceRows = @()
    $since = $null
    if ($Csv -and (Test-Path $Csv)) {
        $snapshots = @(Import-Csv $Csv)
        $since = $snapshots.snapshot | Where-Object { $_ -lt $today } | Sort-Object -Unique | Select-Object -Last 1
        if ($since) {
            $before = @{}
            $snapshots | Where-Object snapshot -eq $since | ForEach-Object { $before[$_.tag] = $_ }
            $sinceRows = @($releases | ForEach-Object {
                $b = $before[$_.tag]
                $installer = $_.installer - $(if ($b) { [int]$b.installer } else { 0 })
                $checks = $_.checks - $(if ($b) { [int]$b.checks } else { 0 })
                if ($installer -ne 0 -or $checks -ne 0) {
                    [pscustomobject]@{
                        Release   = $_.tag + $(if ($_.prerelease) { ' (RC)' } else { '' }) + $(if (-not $b) { ' (new)' } else { '' })
                        Published = ([datetime]$_.published).ToString('yyyy-MM-dd')
                        Installer = '+' + $installer
                        Checks    = '+' + $checks
                    }
                }
            })
        }
    }
    $newInstaller = ($sinceRows | ForEach-Object { [int]($_.Installer.TrimStart('+')) } | Measure-Object -Sum).Sum
    $newChecks = ($sinceRows | ForEach-Object { [int]($_.Checks.TrimStart('+')) } | Measure-Object -Sum).Sum
    $totalInstaller = ($stable | Measure-Object installer -Sum).Sum
    $totalChecks = ($stable | Measure-Object checks -Sum).Sum

    $md = @(
        "# RIGStats downloads — $week"
        ''
        "Snapshot $today (UTC). Newest release: **$($stable[0].tag)** ($(([datetime]$stable[0].published).ToString('yyyy-MM-dd')))."
        ''
        '## The week in numbers'
        ''
        "- **Update checks per day:** $($current.'Checks/day')" + $(if ($last) { " ($($current.Change) from $($last.Week))" } else { '' })
        "- **Active installations (estimate):** $($current.Active)"
        $(if ($since) { "- **Installer downloads since $($since):** $newInstaller (update checks: $newChecks)" } else { '- **Installer downloads this week:** no earlier snapshot to compare with' })
        "- **All time (releases, not RCs):** $totalInstaller installer downloads, $totalChecks update checks"
        ''
        'An installation checks for updates 2–5 times a day (at start, then every 6 h), so "active" is a range, the owner''s own PCs included. Installer downloads are new installs, winget, the app''s own updates and scanners — not people.'
        ''
        $(if ($since) { "## Downloads per release since $since" } else { '## Downloads per release' })
        ''
        $(if ($since) { ConvertTo-MarkdownTable $sinceRows } else { '_The next report compares with this snapshot._' })
        ''
        '## Update checks per day, the last 12 weeks'
        ''
        (ConvertTo-MarkdownTable $recent)
        ''
        'Every week since the start: `pwsh -File tools\download-stats.ps1 -Trend`.'
        ''
        '## Totals per minor version'
        ''
        (ConvertTo-MarkdownTable (Get-ByMinor))
        ''
        '_Written by `tools/download-stats.ps1 -Report` (`.github/workflows/download-stats.yml`); the numbers behind it are in `downloads.csv`._'
    )
    New-Item -ItemType Directory -Force $Report | Out-Null
    $text = ($md -join "`n") + "`n"
    foreach ($name in "$week.md", 'latest.md') {
        $text | Set-Content (Join-Path $Report $name) -Encoding utf8NoBOM -NoNewline
    }
    Write-Host "Wrote $(Join-Path $Report "$week.md") and latest.md"
    return
}

Get-PerRelease $Top | Format-Table -AutoSize
$totalInstaller = ($stable | Measure-Object installer -Sum).Sum
$totalChecks = ($stable | Measure-Object checks -Sum).Sum
"{0} releases — installer downloads: {1}, update checks: {2} (release candidates not counted)" -f $stable.Count, $totalInstaller, $totalChecks
