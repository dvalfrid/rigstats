# Puts the release's highlights (release-highlights.md) into the release:
#
#   tools\release-highlights.ps1 -Version 1.48.0 -Notes <text>
#       → the notes with a "### Highlights" section first under the version's
#         heading (for latest.json, so the app's Updates window shows them).
#   tools\release-highlights.ps1 -Version 1.48.0 -Body
#       → a "## Highlights" block for the top of the GitHub Release text.
#
# Only a list for -Version counts (the file's "# <version>" heading), so last
# release's list never shows up again; without one the notes come back as
# they were and -Body gives nothing. HTML comments (the file's instructions)
# are skipped. Mirrors update_check::with_highlights in the app, which shows
# the bundled copy. Used by .github/workflows/release.yml; see "Release
# routine" in docs/release.md.
param(
    [Parameter(Mandatory)][string]$Version,
    [string]$Notes,
    [switch]$Body,
    [string]$Path = (Join-Path $PSScriptRoot '..\release-highlights.md')
)

$items = @()
if (Test-Path $Path) {
    $text = [regex]::Replace((Get-Content $Path -Raw), '(?s)<!--.*?(-->|$)', '')
    $lines = $text -split "`r?`n" | ForEach-Object { $_.Trim() }
    $heading = $lines | Where-Object { $_ -like '# *' } | Select-Object -First 1
    $forVersion = $heading -and (($heading.Substring(2).Trim() -replace '^v', '') -eq $Version)
    if ($forVersion) { $items = @($lines | Where-Object { $_ -like '- *' }) }
}

if ($Body) {
    if ($items.Count -gt 0) {
        "## Highlights`n`n" + ($items -join "`n") + "`n`n---`n"
    }
    return
}

if ($items.Count -eq 0) { return $Notes }
$escaped = [regex]::Escape($Version)
$heading = [regex]::Match($Notes, "(?m)^## \[?v?$escaped(?![0-9A-Za-z.]).*$")
if (-not $heading.Success) { return $Notes }
$at = $heading.Index + $heading.Length
$Notes.Substring(0, $at) + "`n`n### Highlights`n`n" + ($items -join "`n") + "`n" + $Notes.Substring($at)
