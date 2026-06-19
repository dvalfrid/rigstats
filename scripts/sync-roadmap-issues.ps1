# Sync ROADMAP features to GitHub Issues + the v2.0 milestone.
#
# Idempotent UPSERT keyed by a hidden marker in each issue body:
#     <!-- roadmap-id: <id> -->
# Re-running never duplicates: issues are matched by marker (and, on the first
# run after adopting the original ad-hoc issues, by exact title as a fallback),
# then title/body/label/milestone/state are reconciled to match the data below.
#
# Source of truth = the $features array. ROADMAP.md stays the human-readable doc.
# Run:  pwsh -NoProfile -File scripts/sync-roadmap-issues.ps1
$ErrorActionPreference = "Stop"
$gh = "C:\Program Files\GitHub CLI\gh.exe"
$ms = "v2.0"

function Norm([string]$s) { if ($null -eq $s) { return "" } ($s -replace "`r`n", "`n").Trim() }

# 1. Ensure milestone exists
$msAll = & $gh api "repos/:owner/:repo/milestones?state=all" | ConvertFrom-Json
$msObj = $msAll | Where-Object { $_.title -eq $ms } | Select-Object -First 1
if (-not $msObj) {
  $msObj = & $gh api "repos/:owner/:repo/milestones" -f title="$ms" -f state="open" `
    -f description="RIGStats 2.0 - full roadmap (shipped + planned)" | ConvertFrom-Json
  Write-Host "Created milestone '$ms' (#$($msObj.number))."
}

# 2. Feature data.  kind = done | dropped | planned.
#    pin = adopt an existing issue by number WITHOUT rewriting its title/body
#          (used for the two pre-existing issues #81/#83 we only want to track).
$features = @(
  @{ id="auto-update"; kind="done"; label="enhancement"; title="Auto-update";
     summary="Silent update check on startup (10s delay) then every 6h. A header badge appears when a newer version is available; the updater dialog shows GitHub release notes, full local version history, and a download progress bar, then auto-restarts.";
     status="Shipped in v1.6.0 - 41b8223b" }
  @{ id="nvme-ssd-temperatures"; kind="done"; label="enhancement"; title="NVMe / SSD temperatures";
     summary="Each drive in the Disk panel shows a live temperature, identified via LHM SensorId prefixes (/nvme/, /ssd/, ...) and matched to drives by model name so inserting a USB drive never shifts readings.";
     status="Shipped in v1.8.0 - 3839e92b" }
  @{ id="temperature-threshold-alerts"; kind="done"; label="enhancement"; title="Temperature threshold alerts";
     summary="Configurable per-component warn/crit thresholds (CPU/GPU/RAM/Disk) fire Windows tray notifications, with independent warn/crit cooldowns and one-time migration to the thresholds map.";
     status="Shipped in v1.9.0 - d77d1e42" }
  @{ id="motherboard-panel"; kind="done"; label="enhancement"; title="Motherboard panel";
     summary="Opt-in panel showing the Super I/O chip's fans, temperatures, and named voltage rails plus the detected board name, using the chip-agnostic /lpc/ SensorId prefix.";
     status="Shipped in v1.11.0 - d927bc40" }
  @{ id="extended-gpu-panel"; kind="done"; label="enhancement"; title="Extended GPU panel";
     summary="GPU panel extended to TEMP / HOT / FREQ / POWER with hotspot thresholds and a two-row bar layout (VRAM+3D, FAN+VDEC) when D3D data is present.";
     status="Shipped in v1.13.0 - dd3a5da1" }
  @{ id="customisable-themes"; kind="done"; label="enhancement"; title="Customisable themes / accent colours";
     summary="Five built-in accent presets selectable in Settings -> Appearance with live preview; the full accent palette and tonal label variants are derived from a single accent via HSL.";
     status="Shipped in v1.14.0 - 81447757" }
  @{ id="process-monitor-panel"; kind="done"; label="enhancement"; title="Process monitor panel";
     summary="Opt-in panel showing the top 8 processes by CPU% with RAM usage, refreshed each tick. Process names are HTML-escaped before display.";
     status="Shipped in v1.15.0 - 9cd1aa64" }
  @{ id="floating-panel-layout"; kind="done"; label="enhancement"; title="Floating panel layout";
     summary="Floating mode opens each visible panel as its own frameless, always-on-top window, positionable across any number of monitors with positions persisted across restarts.";
     status="Shipped in v1.16.0 - 97dcc7a6" }
  @{ id="multi-gpu-selector"; kind="done"; label="enhancement"; title="Multi-GPU selector and pinning";
     summary="Systems with both iGPU and dGPU can pin which GPU the panel shows via selector dots; the preference persists, with a stable highest-VRAM default to avoid per-tick switching.";
     status="Shipped in v1.19.0 - fae0a0b4" }
  @{ id="battery-panel"; kind="done"; label="enhancement"; title="Battery panel (laptop support)";
     summary="Laptop battery panel (charge %, charging state, time remaining, power draw) via WMI Win32_Battery; renders a NO BATTERY state on desktops so it is always safe to enable.";
     status="Shipped in v1.20.0 - 6089ee8d" }
  @{ id="settings-redesign"; kind="done"; label="enhancement"; title="Settings redesign";
     summary="Settings reorganised from a tall two-column layout into a compact four-tab interface (Dashboard / Panels / Alerts / Appearance). Battery charge alert thresholds added alongside.";
     status="Shipped in v1.20.0 - 6089ee8d" }
  @{ id="lhm-sensor-sidecar"; kind="done"; label="enhancement"; title="LHM stability - sensor sidecar";
     summary="Replaced the LibreHardwareMonitor HTTP server with a managed .NET 10 Windows-service sidecar streaming sensor JSON over a named pipe; the Rust backend connects as a read-only pipe client.";
     status="Shipped in v1.21.0 - 7a9e063b" }
  @{ id="desktop-background-l1"; kind="done"; label="enhancement"; title="Desktop background - Level 1 (HWND_BOTTOM)";
     summary="Three-way Window Layer selector (Normal / Always On Top / Always Behind). Behind mode uses SetWindowPos(HWND_BOTTOM) with focus-based re-pinning; applies to the main window and floating panels.";
     status="Shipped in v1.24.0 - bfaaea2d" }
  @{ id="egui-migration"; kind="done"; label="enhancement"; title="egui migration - replace Tauri/WebView2 with native egui";
     summary="Replaced the Tauri/WebView2 frontend with native egui/eframe across all panels, settings, updater, tray, and brand logos. Idle CPU dropped from ~2-4% (WebView2) to ~0% between repaints.";
     status="Shipped in v1.27.0 - 650139a7" }
  @{ id="stats-logging"; kind="done"; label="enhancement"; title="Stats logging / data export";
     summary="Opt-in CSV stats logging: one row per tick to a daily rolling file with automatic retention pruning, a Settings card (enable + retention + open folder), and a tray Start/Stop Recording toggle with a red-dot icon.";
     status="Shipped in - 696352b2" }
  @{ id="remove-nodejs-npm"; kind="done"; label="enhancement"; title="Remove Node.js / npm infrastructure";
     summary="Removed all Node.js/npm infrastructure following the egui migration (package.json, node_modules, vitest, ESLint, lefthook). The build pipeline is now pure cargo + dotnet.";
     status="Shipped in - 88850380" }

  @{ id="cpu-fan-speed"; kind="dropped"; label="wontfix"; title="CPU fan speed";
     summary="Investigated: LHM exposes cooler fans as generic Fan #N channels with no signal identifying which channel is the CPU cooler, and a highest-RPM heuristic proved unreliable. CPU cooler RPM remains available in the Motherboard panel.";
     status="Not planned - investigated, no reliable signal" }
  @{ id="background-transparency"; kind="dropped"; label="wontfix"; title="Background-only transparency (per-pixel alpha)";
     summary="Goal was transparent panel backgrounds with opaque text/graphs. All four attempted approaches (wgpu/glow transparent, two LWA_COLORKEY variants) failed; achieving it requires custom Win32 DirectComposition integration (1-2 weeks, high risk).";
     status="Not planned - blocked, needs DirectComposition" }
  @{ id="ui-performance-strategy"; kind="dropped"; label="wontfix"; title="UI performance - lighter rendering strategy";
     summary="The WebView2 DOM render cost this aimed to reduce is gone entirely after the egui migration; the egui binary sleeps between repaints and idles at ~0% CPU.";
     status="Not planned - superseded by the egui migration (v1.27)" }

  @{ id="floating-panel-groups"; kind="planned"; label="enhancement"; title="Floating panel groups";
     summary="Build on floating mode: magnetically snap panels into groups that move together (vertical or horizontal), with a flip-orientation context action and a 'Collect panels to screen' tray command.";
     status="Planned" }
  @{ id="floating-mode-perf"; kind="planned"; label="enhancement"; title="Floating mode - reduce multi-window rendering cost";
     summary="Floating mode renders one OS viewport per panel via show_viewport_immediate, so cost scales with panel count. Investigate deferred viewports and skipping unchanged panels. Target: idle CPU within ~2x of fixed mode.";
     status="Planned" }
  @{ id="desktop-background-l2"; kind="planned"; label="enhancement"; title="Desktop background - Level 2 (WorkerW)";
     summary="True wallpaper-layer mode that reparents into the Progman/WorkerW hierarchy so the dashboard lives between wallpaper and icons and survives Win+D. Relies on undocumented Windows internals and needs Explorer-restart handling plus input forwarding.";
     status="Planned" }
  @{ id="streamdeck"; kind="planned"; label="enhancement"; title="Stream Deck integration";
     summary="Display live hardware stats (CPU/GPU load/temp, VRAM, fan RPM) on Stream Deck keys directly over USB HID via the elgato-streamdeck crate - no Elgato software or HTTP server. Opt-in; requires the Elgato app not be running.";
     status="Planned" }
  @{ id="total-system-power"; kind="planned"; label="enhancement"; title="Total system power consumption";
     summary="Show total real-time system power draw: use a motherboard power sensor if LHM exposes one, otherwise a labelled component-sum estimate. Laptops are already covered by battery discharge rate.";
     status="Planned" }
  @{ id="landscape-support"; kind="planned"; label="enhancement"; title="Landscape monitor support";
     summary="Add landscape profiles (1920x1080, ultrawide strip, etc.) with a horizontal flow layout so users with a landscape or ultrawide secondary monitor can run the dashboard.";
     status="Planned" }
  @{ id="post-update-notification"; kind="planned"; label="enhancement"; title="Post-update success notification";
     summary="After a silent in-app update, show an 'Updated to vX' confirmation in the updater dialog, driven by a PROGRAMDATA flag file the NSIS installer writes and the app checks at startup.";
     status="Planned" }
  @{ id="test-coverage-sidecar"; kind="planned"; label="enhancement"; title="Test coverage - sidecar + sensor extraction";
     summary="Add tests for the currently untested .NET sensor sidecar (named-pipe framing, JSON serialization, LHM sensor mapping) and direct unit tests for the lhm.rs extract_* filtering edge cases.";
     status="Planned" }

  # Pre-existing issues: track only (do not rewrite their title/body).
  @{ id="gpu-driver-warning"; kind="done"; pin=81 }
  @{ id="fullscreen-mode"; kind="done"; pin=83 }
)

# 3. Fetch all issues once; index by marker and by title.
$all = & $gh issue list --state all --limit 500 --json number,title,body,state,stateReason,labels,milestone | ConvertFrom-Json
$byMarker = @{}
$byTitle = @{}
$markerRe = [regex]'<!--\s*roadmap-id:\s*([a-z0-9-]+)\s*-->'
foreach ($i in $all) {
  $m = $markerRe.Match([string]$i.body)
  if ($m.Success) { $byMarker[$m.Groups[1].Value] = $i }
  if (-not $byTitle.ContainsKey($i.title)) { $byTitle[$i.title] = $i }
}

$knownIds = @{}
$stats = @{ created = 0; adopted = 0; updated = 0; unchanged = 0; stateFixed = 0 }

foreach ($f in $features) {
  $knownIds[$f.id] = $true
  $marker = "<!-- roadmap-id: $($f.id) -->"
  $desiredState = if ($f.kind -eq "planned") { "OPEN" } else { "CLOSED" }
  $closeReason = if ($f.kind -eq "dropped") { "not planned" } else { "completed" }

  # ---- locate the issue ----
  $issue = $null
  if ($f.ContainsKey("pin")) {
    $issue = $all | Where-Object { $_.number -eq $f.pin } | Select-Object -First 1
  } elseif ($byMarker.ContainsKey($f.id)) {
    $issue = $byMarker[$f.id]
  } elseif ($byTitle.ContainsKey($f.title)) {
    $issue = $byTitle[$f.title]   # adopt the originally-created issue
  }

  # ---- create if missing ----
  if (-not $issue) {
    $body = "$($f.summary)`n`nStatus: $($f.status)`nSource: ROADMAP.md`n`n$marker"
    $url = & $gh issue create --title $f.title --body $body --label $f.label --milestone $ms
    $num = ($url | Select-String -Pattern '(\d+)\s*$').Matches.Groups[1].Value
    if ($desiredState -eq "CLOSED") { & $gh issue close $num --reason $closeReason | Out-Null }
    Write-Host "CREATED  #$num  $($f.title)"
    $stats.created++
    continue
  }

  $num = $issue.number

  # ---- pinned: ensure marker present + milestone + state only ----
  if ($f.ContainsKey("pin")) {
    if (-not $markerRe.Match([string]$issue.body).Success) {
      $newBody = (Norm $issue.body) + "`n`n$marker"
      & $gh issue edit $num --body $newBody | Out-Null
      Write-Host "STAMPED  #$num  (marker added)"
      $stats.adopted++
    }
    if (($issue.milestone.title) -ne $ms) { & $gh issue edit $num --milestone $ms | Out-Null }
    continue
  }

  # ---- managed issue: reconcile title/body/label/milestone/state ----
  $adoptedNow = -not $markerRe.Match([string]$issue.body).Success
  $desiredBody = "$($f.summary)`n`nStatus: $($f.status)`nSource: ROADMAP.md`n`n$marker"
  $editArgs = @()
  if ($issue.title -ne $f.title) { $editArgs += @("--title", $f.title) }
  if ((Norm $issue.body) -ne (Norm $desiredBody)) { $editArgs += @("--body", $desiredBody) }
  if (($issue.milestone.title) -ne $ms) { $editArgs += @("--milestone", $ms) }
  $hasLabel = @($issue.labels | Where-Object { $_.name -eq $f.label }).Count -gt 0
  if (-not $hasLabel) { $editArgs += @("--add-label", $f.label) }

  if ($editArgs.Count -gt 0) {
    & $gh issue edit $num @editArgs | Out-Null
    if ($adoptedNow) { Write-Host "ADOPTED  #$num  $($f.title)"; $stats.adopted++ }
    else { Write-Host "UPDATED  #$num  $($f.title)"; $stats.updated++ }
  } else {
    $stats.unchanged++
  }

  # ---- state reconcile ----
  if ($issue.state -ne $desiredState) {
    if ($desiredState -eq "CLOSED") { & $gh issue close $num --reason $closeReason | Out-Null }
    else { & $gh issue reopen $num | Out-Null }
    Write-Host "STATE    #$num  -> $desiredState"
    $stats.stateFixed++
  }
}

# 4. Report orphans: managed-looking issues whose id is no longer in the data.
foreach ($id in $byMarker.Keys) {
  if (-not $knownIds.ContainsKey($id)) {
    Write-Host "ORPHAN   #$($byMarker[$id].number)  roadmap-id '$id' not in data (review manually)"
  }
}

Write-Host "`nSummary: created=$($stats.created) adopted=$($stats.adopted) updated=$($stats.updated) state-fixed=$($stats.stateFixed) unchanged=$($stats.unchanged)"
