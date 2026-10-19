# Thanks the people whose work is in a release, for the GitHub Release text:
#
#   tools\release-contributors.ps1 -Tag v1.51.0 -Repo dvalfrid/rigstats -Owner dvalfrid
#       → a "## Thanks" block, or nothing when only the owner and bots
#         contributed.
#
# Who: the authors of the commits since the previous release (a squash merge
# makes the pull request's author the commit's author) and the people in
# their "Co-authored-by:" lines with a GitHub noreply address — not the
# owner, not bots (dependabot, Copilot, github-actions, release-please).
# Each with the pull requests their commits came from ("… (#471)" at the end
# of the subject). The previous release is the newest real release (not a
# pre-release) older than -Tag. Used by .github/workflows/release.yml; see
# "Release routine" in docs/release.md.
param(
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$Repo,
    [Parameter(Mandatory)][string]$Owner
)

function VersionOf([string]$t) {
    [version](($t -replace '^v', '') -replace '-.*$', '')
}

$current = VersionOf $Tag
$previous = gh release list --repo $Repo --exclude-drafts --exclude-pre-releases --limit 50 --json tagName |
    ConvertFrom-Json |
    Where-Object { $_.tagName -ne $Tag -and (VersionOf $_.tagName) -lt $current } |
    Sort-Object { VersionOf $_.tagName } -Descending |
    Select-Object -First 1
if (-not $previous) { return }

$commits = (gh api "repos/$Repo/compare/$($previous.tagName)...$Tag" | ConvertFrom-Json).commits

function IsPerson([string]$login, [string]$type) {
    $login -and $login -ne $Owner -and $type -ne 'Bot' -and $login -notmatch '\[bot\]$' -and $login -ne 'Copilot'
}

$people = [ordered]@{}
foreach ($c in $commits) {
    $message = $c.commit.message
    $subject = ($message -split "`n")[0]
    $pr = if ($subject -match '\(#(\d+)\)\s*$') { "#$($Matches[1])" } else { $null }
    $logins = @()
    if (IsPerson $c.author.login $c.author.type) { $logins += $c.author.login }
    # Co-authors with a GitHub noreply address name their login in it:
    # "12345+login@users.noreply.github.com" or "login@users.noreply.github.com".
    foreach ($m in [regex]::Matches($message, '(?im)^Co-authored-by:.*<(?:\d+\+)?([^@<>\s]+)@users\.noreply\.github\.com>')) {
        $login = $m.Groups[1].Value
        if (IsPerson $login 'User') { $logins += $login }
    }
    foreach ($login in $logins | Select-Object -Unique) {
        if (-not $people.Contains($login)) { $people[$login] = [System.Collections.Generic.List[string]]::new() }
        if ($pr -and -not $people[$login].Contains($pr)) { $people[$login].Add($pr) }
    }
}
if ($people.Count -eq 0) { return }

'## Thanks'
''
'Thanks to everyone outside the core team whose work is in this release:'
''
foreach ($login in $people.Keys) {
    $prs = $people[$login]
    if ($prs.Count -gt 0) { "- @$login — $($prs -join ', ')" } else { "- @$login" }
}
''
