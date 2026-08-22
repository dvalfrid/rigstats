# Sync ROADMAP features to GitHub Issues + the v2.0 milestone.
#
# Idempotent UPSERT keyed by a hidden marker in each issue body:
#     <!-- roadmap-id: <id> -->
# Re-running never duplicates: issues are matched by marker (and, on the first
# run after adopting the original ad-hoc issues, by exact title as a fallback),
# then title/body/label/milestone/state are reconciled to match the data below.
#
# Source of truth = the $features array. ROADMAP.md stays the human-readable doc.
# Run:  pwsh -NoProfile -File tools/sync-roadmap-issues.ps1
$ErrorActionPreference = "Stop"
$gh = "C:\Program Files\GitHub CLI\gh.exe"
$defaultMs = "v2.0"
# Milestones to ensure exist, with descriptions. Features default to $defaultMs;
# a feature opts into a later milestone via its `milestone` key.
$msDesc = [ordered]@{
  "v2.0" = "RIGStats 2.0 - full roadmap (shipped + planned)"
  "v3.0" = "RIGStats 3.0 - future scope (post-2.0 features)"
}

function Norm([string]$s) { if ($null -eq $s) { return "" } ($s -replace "`r`n", "`n").Trim() }

# 1. Ensure every milestone exists; index by title.
$msAll = & $gh api "repos/:owner/:repo/milestones?state=all" | ConvertFrom-Json
$msByTitle = @{}
foreach ($m in $msAll) { $msByTitle[$m.title] = $m }
foreach ($t in $msDesc.Keys) {
  if (-not $msByTitle.ContainsKey($t)) {
    $obj = & $gh api "repos/:owner/:repo/milestones" -f title="$t" -f state="open" `
      -f description="$($msDesc[$t])" | ConvertFrom-Json
    $msByTitle[$t] = $obj
    Write-Host "Created milestone '$t' (#$($obj.number))."
  }
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
  @{ id="background-transparency"; kind="done"; label="enhancement"; title="Background-only transparency (per-pixel alpha)";
     summary="Transparent panel backgrounds with opaque text/graphs/bars/images, in the Normal/Always-on-Top/Always-Behind window layers (Settings/About/Updater dialogs stay fully opaque). Originally dropped as blocked - four approaches failed (wgpu/glow transparent, two LWA_COLORKEY variants). Unblocked by #131/#168: main.rs forces the DX12 backend with wgpu::Dx12SwapchainKind::DxgiFromVisual, applies WS_EX_NOREDIRECTIONBITMAP post-window-creation (win_opacity::set_no_redirection_bitmap), and premultiplies clear_color/panel fills by opacity instead of the old WS_EX_LAYERED whole-window dim. Verified visually in Normal/Always-on-Top/Always-Behind and confirmed live that Settings/About/Status/Updater dialogs stay fully opaque. Floating mode is a separate follow-up: see background-transparency-floating.";
     status="Shipped - main.rs 14da1ec07" }
  @{ id="background-transparency-floating"; kind="planned"; label="enhancement"; title="Extend selective per-pixel transparency (DComp) to floating mode"; milestone="v3.0";
     summary="Floating mode renders one separate OS viewport per visible panel (show_viewport_immediate). Attempted 2026-08-21: applying win_opacity::set_no_redirection_bitmap to a floating panel's HWND renders it as a washed-out light gray/blue instead of the intended translucent panel, even with zero opacity/transparency logic in the drawn content (isolated via controlled tests - not a WS_EX_LAYERED conflict, not a premultiply bug). Points to a real eframe 0.34.3/egui-wgpu limitation in how the swap-chain surface is created for non-root show_viewport_immediate viewports combined with a DComp (DxgiFromVisual) backend. Reverted; floating panels keep the old WS_EX_LAYERED uniform-dim opacity, unaffected. Needs its own dedicated investigation before retrying - see issue comments.";
     status="Blocked - eframe/egui-wgpu secondary-viewport DComp limitation, needs dedicated investigation" }
  @{ id="ui-performance-strategy"; kind="dropped"; label="wontfix"; title="UI performance - lighter rendering strategy";
     summary="The WebView2 DOM render cost this aimed to reduce is gone entirely after the egui migration; the egui binary sleeps between repaints and idles at ~0% CPU.";
     status="Not planned - superseded by the egui migration (v1.27)" }

  @{ id="floating-panel-groups"; kind="planned"; label="enhancement"; title="Floating panel groups"; milestone="v3.0";
     summary="Build on floating mode: magnetically snap panels into groups that move together (vertical or horizontal), with a flip-orientation context action and a 'Collect panels to screen' tray command.";
     status="Planned (3.0)" }
  @{ id="floating-mode-perf"; kind="dropped"; label="wontfix"; title="Floating mode - reduce multi-window rendering cost";
     summary="Investigated via architectural review: the low-hanging fruit (Behind-mode throttle) already shipped separately. The remainder (Arc-wrapping shared state, deferred viewports) is high implementation cost for a sub-1% CPU saving in release builds on the target hardware (gaming rigs); show_viewport_immediate + &-reference is the correct design for panels sharing one 1 Hz data snapshot.";
     status="Not planned - deferred indefinitely unless floating-mode CPU complaints arise on battery systems" }
  @{ id="desktop-background-l2"; kind="done"; label="enhancement"; title="Desktop background - Level 2 (WorkerW)";
     summary="True wallpaper-layer mode (Settings -> Window Layer -> Desktop Wallpaper). A separate rigstats-wallpaper host process reparents into the Progman/WorkerW hierarchy so the dashboard lives between wallpaper and icons and survives Win+D; the main app supervises it (relaunch on Explorer restart) and pauses its own polling to hand the sensor pipe over. Display-only in v1.";
     status="Shipped" }
  @{ id="desktop-background-we-hosted"; kind="planned"; label="enhancement"; title="Desktop background - WE Application wallpaper"; milestone="v3.0";
     summary="Ship the rigstats-wallpaper host as a Wallpaper Engine Application wallpaper so WE handles the WorkerW reparenting, multi-monitor placement and Explorer-restart recovery. Adds a 32-bit build of the host (WE requires a 32-bit single-window app) and a hosted launch mode that fills the rect WE assigns. WE-owners only; mutually exclusive with the built-in WorkerW mode.";
     status="Planned (3.0)" }
  @{ id="streamdeck"; kind="planned"; label="enhancement"; title="Stream Deck integration"; milestone="v3.0";
     summary="Display live hardware stats (CPU/GPU load/temp, VRAM, fan RPM) on Stream Deck keys directly over USB HID via the elgato-streamdeck crate - no Elgato software or HTTP server. Opt-in; requires the Elgato app not be running.";
     status="Planned (3.0)" }
  @{ id="cross-platform-port"; kind="planned"; label="enhancement"; title="Cross-platform OS abstraction - Linux port"; milestone="v3.0";
     summary="Make the RIGStats core OS-agnostic (ports-and-adapters refactor) so it can be ported to Linux. Extract a win-free rigstats-core defining trait ports (SensorProvider, HardwareProbe, SystemPaths, Autostart, WindowPlatform) and a Platform facade; move WMI/registry/named-pipe/win32 code into a platform-windows adapter crate; add a platform-linux crate (sysfs/NVML/dbus/unix-socket). Replace hard-coded %APPDATA% with the directories crate and the named pipe with interprocess. LHM stays a Windows-only adapter. Per-OS packaging: NSIS/sc on Windows, .deb/AppImage + systemd on Linux.";
     status="Planned (3.0)" }
  @{ id="total-system-power"; kind="done"; label="enhancement"; title="Total system power consumption";
     summary="Show total real-time system power draw: use a motherboard power sensor if LHM exposes one, otherwise a labelled component-sum estimate. Laptops are already covered by battery discharge rate.";
     status="Done (v1.34)" }
  @{ id="landscape-support"; kind="done"; label="enhancement"; title="Landscape monitor support";
     summary="Landscape profiles (transposed portrait sizes plus Top variants) render every panel in an adaptive grid sized to the profile, auto-target a matching monitor (else the primary screen), and support the pinnable non-floating window.";
     status="Shipped in v1.32.0" }
  @{ id="post-update-notification"; kind="done"; label="enhancement"; title="Post-update success notification";
     summary="After an in-app update the NSIS /autoupdate installer relaunches the app with --just-updated=VERSION, and the app shows an 'Updated to vX' confirmation in the updater dialog at startup.";
     status="Shipped" }
  @{ id="test-coverage-sidecar"; kind="done"; label="enhancement"; title="Test coverage - sidecar + sensor extraction";
     summary="Add tests for the currently untested .NET sensor sidecar (named-pipe framing, JSON serialization, LHM sensor mapping) and direct unit tests for the lhm.rs extract_* filtering edge cases.";
     status="Shipped in v2.0" }
  @{ id="session-history"; kind="planned"; label="enhancement"; title="Session history: record, browse, and visualize past sessions"; milestone="v3.0";
     summary="Turn stats logging into named, browsable recording sessions: explicit start/stop via the existing tray Start/Stop Recording toggle, a new per-session CSV file instead of the daily-rolling log, and a History window (egui_plot charts of CPU/GPU/RAM/disk/net/ping) to pick a past session and see what happened. Sessions can be pinned to survive retention pruning, renamed, and deleted.";
     status="Planned (3.0)" }

  # Pre-existing issues: track only (do not rewrite their title/body).
  # `title` here is display-only (used for the ROADMAP table; the pinned branch
  # never rewrites the issue's real title).
  @{ id="gpu-driver-warning"; kind="done"; pin=81; title="GPU driver version + stale-driver warning" }
  @{ id="fullscreen-mode"; kind="done"; pin=83; title="Fullscreen (fill-screen) mode" }
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
$stats = @{ created = 0; adopted = 0; updated = 0; unchanged = 0; stateFixed = 0; drift = 0 }
$rows = @()   # collected for the auto-generated ROADMAP.md table

foreach ($f in $features) {
  $knownIds[$f.id] = $true
  $fMs = if ($f.ContainsKey("milestone")) { $f.milestone } else { $defaultMs }
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
    $url = & $gh issue create --title $f.title --body $body --label $f.label --milestone $fMs
    $num = ($url | Select-String -Pattern '(\d+)\s*$').Matches.Groups[1].Value
    if ($desiredState -eq "CLOSED") { & $gh issue close $num --reason $closeReason | Out-Null }
    Write-Host "CREATED  #$num  $($f.title)"
    $stats.created++
    $rows += @{ id = $f.id; num = [int]$num; kind = $f.kind; title = $f.title; ms = $fMs }
    continue
  }

  $num = $issue.number
  $rows += @{ id = $f.id; num = [int]$num; kind = $f.kind; title = $f.title; ms = $fMs }

  # ---- pinned: ensure marker present + milestone + state only ----
  if ($f.ContainsKey("pin")) {
    if (-not $markerRe.Match([string]$issue.body).Success) {
      $newBody = (Norm $issue.body) + "`n`n$marker"
      & $gh issue edit $num --body $newBody | Out-Null
      Write-Host "STAMPED  #$num  (marker added)"
      $stats.adopted++
    }
    if (($issue.milestone.title) -ne $fMs) { & $gh issue edit $num --milestone $fMs | Out-Null }
    continue
  }

  # ---- managed issue: reconcile title/body/label/milestone/state ----
  $adoptedNow = -not $markerRe.Match([string]$issue.body).Success
  $desiredBody = "$($f.summary)`n`nStatus: $($f.status)`nSource: ROADMAP.md`n`n$marker"
  $editArgs = @()
  if ($issue.title -ne $f.title) { $editArgs += @("--title", $f.title) }
  if ((Norm $issue.body) -ne (Norm $desiredBody)) { $editArgs += @("--body", $desiredBody) }
  if (($issue.milestone.title) -ne $fMs) { $editArgs += @("--milestone", $fMs) }
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
    if ($desiredState -eq "CLOSED") {
      & $gh issue close $num --reason $closeReason | Out-Null
      Write-Host "STATE    #$num  -> CLOSED"
      $stats.stateFixed++
    } elseif ($issue.stateReason -eq "COMPLETED") {
      # GitHub auto-closed this as completed (e.g. a commit landed with "Closes #N")
      # but $features still marks it planned. Reopening would resurrect finished
      # work, so we refuse and flag drift instead: the fix is to set this feature's
      # kind to "done" here. Surfaced as an error so it cannot pass silently.
      Write-Host "DRIFT    #$num  '$($f.id)' closed as completed but kind=planned -> NOT reopened; set kind='done' in `$features"
      $stats.drift++
    } else {
      & $gh issue reopen $num | Out-Null
      Write-Host "STATE    #$num  -> OPEN"
      $stats.stateFixed++
    }
  }
}

# 4. Report orphans: managed-looking issues whose id is no longer in the data.
foreach ($id in $byMarker.Keys) {
  if (-not $knownIds.ContainsKey($id)) {
    Write-Host "ORPHAN   #$($byMarker[$id].number)  roadmap-id '$id' not in data (review manually)"
  }
}

# 5. Regenerate the ROADMAP.md issue-tracking table between its markers.
#    Grouped Done -> Not planned -> Planned, each sorted by issue number.
$repo = "https://github.com/dvalfrid/rigstats/issues"
$statusText = @{ done = "✅ Done"; dropped = "⏭ Not planned"; planned = "🔲 Planned" }
$kindRank = @{ done = 0; dropped = 1; planned = 2 }
$sorted = $rows | Sort-Object @{ Expression = { $kindRank[$_.kind] } }, @{ Expression = { $_.num } }
$tableLines = @(
  "| Issue | roadmap-id | Feature | Milestone | Status |"
  "| --- | --- | --- | --- | --- |"
)
foreach ($r in $sorted) {
  $tableLines += "| [#$($r.num)]($repo/$($r.num)) | ``$($r.id)`` | $($r.title) | $($r.ms) | $($statusText[$r.kind]) |"
}

$roadmapPath = Join-Path $PSScriptRoot "..\ROADMAP.md"
$raw = Get-Content -Raw -LiteralPath $roadmapPath
$nl = if ($raw -match "`r`n") { "`r`n" } else { "`n" }
$block = "<!-- roadmap-table:start -->$nl" + ($tableLines -join $nl) + "$nl<!-- roadmap-table:end -->"
$rx = [regex]'(?s)<!-- roadmap-table:start -->.*?<!-- roadmap-table:end -->'
if (-not $rx.IsMatch($raw)) {
  Write-Host "WARN  ROADMAP.md has no <!-- roadmap-table:start/end --> markers; table not written."
} else {
  $updated = $rx.Replace($raw, [System.Text.RegularExpressions.MatchEvaluator] { param($m) $block })
  if ((($updated -replace "`r`n", "`n")) -ne (($raw -replace "`r`n", "`n"))) {
    [System.IO.File]::WriteAllText($roadmapPath, $updated)
    Write-Host "ROADMAP  table regenerated ($($sorted.Count) rows)."
  } else {
    Write-Host "ROADMAP  table already up to date."
  }
}

Write-Host "`nSummary: created=$($stats.created) adopted=$($stats.adopted) updated=$($stats.updated) state-fixed=$($stats.stateFixed) unchanged=$($stats.unchanged) drift=$($stats.drift)"

if ($stats.drift -gt 0) {
  Write-Host "`nERROR: $($stats.drift) issue(s) closed as completed but still marked kind='planned' in `$features." -ForegroundColor Red
  Write-Host "Update those entries to kind='done' (and the ROADMAP status) so the data matches reality, then re-run." -ForegroundColor Red
  exit 1
}
