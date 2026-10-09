# Start-up smoke test for the egui app: launches it once per start mode
# (normal, wallpaper, floating, overlay, just-updated), checks windows,
# processes and the debug log, and puts your settings back afterwards.
#
#   pwsh -File tools\smoke-app.ps1                 # all modes, debug build
#   pwsh -File tools\smoke-app.ps1 -Only wallpaper # one mode
#   pwsh -File tools\smoke-app.ps1 -Restart        # relaunch the installed app at the end
#
# Stops every running rigstats.exe / rigstats-wallpaper.exe first (the
# installed app included — single-instance guard). Exit code = failed modes.
# See tools/README.md.
param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\target\debug\rigstats.exe'),
    [int]$Wait = 12,
    [string[]]$Only,
    [switch]$Restart
)

$ErrorActionPreference = 'Stop'
$dataDir = Join-Path $env:APPDATA 'se.codeby.rigstats'
$settingsPath = Join-Path $dataDir 'rigstats-settings.json'
$logPath = Join-Path $dataDir 'rigstats-debug.log'
$parked = -30000   # main window parked off-screen (wallpaper / floating)

if (-not (Test-Path $Exe)) { throw "Not found: $Exe — build first (cargo build --manifest-path src-egui/Cargo.toml)" }
$Exe = (Resolve-Path $Exe).Path

Add-Type @'
using System; using System.Text; using System.Runtime.InteropServices; using System.Collections.Generic;
public static class SmokeWin {
  public delegate bool EP(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EP cb, IntPtr l);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [StructLayout(LayoutKind.Sequential)] struct R { public int L, T, Ri, B; }
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out R r);
  public class Win { public string Title; public bool Visible; public int X, Y, W, H; }
  public static List<Win> Of(uint pid) {
    var o = new List<Win>();
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p != pid) return true;
      var s = new StringBuilder(256); GetWindowText(h, s, 256);
      if (s.Length == 0) return true;
      R r; GetWindowRect(h, out r);
      o.Add(new Win { Title = s.ToString(), Visible = IsWindowVisible(h), X = r.L, Y = r.T, W = r.Ri - r.L, H = r.B - r.T });
      return true;
    }, IntPtr.Zero);
    return o;
  }
}
'@

function Stop-RigStats {
    Get-Process rigstats, rigstats-wallpaper -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.Id -Force }
    for ($i = 0; $i -lt 20 -and (Get-Process rigstats, rigstats-wallpaper -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 250 }
}

# Each mode: settings to override, extra arguments, and checks. A check is a
# label and a scriptblock over $r (Main, Windows, HostRunning, Log) that
# returns $true when it holds.
$em = [char]0x2014
$modes = [ordered]@{
    normal = @{
        Settings = @{ windowLayer = 'normal'; floatingMode = $false; overlayEnabled = $false }
        Checks   = [ordered]@{
            'main window on-screen'  = { $r.Main.Visible -and $r.Main.X -gt $parked }
            'no wallpaper host'      = { -not $r.HostRunning }
        }
    }
    wallpaper = @{
        Settings = @{ windowLayer = 'wallpaper'; floatingMode = $false; overlayEnabled = $false }
        Checks   = [ordered]@{
            'main window parked'     = { $r.Main.X -le $parked }
            'wallpaper host running' = { $r.HostRunning }
            'host attached'          = { $r.Log -match 'attached to WorkerW' }
        }
    }
    floating = @{
        Settings = @{ windowLayer = 'normal'; floatingMode = $true; overlayEnabled = $false }
        Checks   = [ordered]@{
            'main window parked'     = { $r.Main.X -le $parked }
            'a floating panel shown' = { @($r.Windows | Where-Object { $_.Visible -and $_.Title -like "RigStats $em *" -and $_.Title -notmatch 'Overlay' }).Count -gt 0 }
            'no wallpaper host'      = { -not $r.HostRunning }
        }
    }
    overlay = @{
        Settings = @{ windowLayer = 'normal'; floatingMode = $false; overlayEnabled = $true }
        Checks   = [ordered]@{
            'main window on-screen'  = { $r.Main.Visible -and $r.Main.X -gt $parked }
            'overlay shown'          = { @($r.Windows | Where-Object { $_.Visible -and $_.Title -eq "RigStats $em Overlay" }).Count -eq 1 }
        }
    }
    'just-updated' = @{
        Settings = @{ windowLayer = 'normal'; floatingMode = $false; overlayEnabled = $false }
        Args     = @('--just-updated=0.0.0-smoke')
        Checks   = [ordered]@{
            'main window on-screen'  = { $r.Main.Visible -and $r.Main.X -gt $parked }
            'update window shown'    = { @($r.Windows | Where-Object { $_.Visible -and $_.Title -eq 'RigStats Update' }).Count -eq 1 }
        }
    }
}
$common = [ordered]@{
    'app still running'     = { $r.Alive }
    'no [ERROR] in the log' = { -not ($r.Log -match '\[ERROR\]') }
    'no panic in the log'   = { -not ($r.Log -match 'panicked') }
}

$names = if ($Only) { $Only } else { $modes.Keys }
$original = [System.IO.File]::ReadAllBytes($settingsPath)
$failed = @()
try {
    foreach ($name in $names) {
        $mode = $modes[$name]
        if (-not $mode) { throw "Unknown mode '$name' (modes: $($modes.Keys -join ', '))" }
        Stop-RigStats
        $s = [System.Text.Encoding]::UTF8.GetString($original) | ConvertFrom-Json
        foreach ($k in $mode.Settings.Keys) { $s | Add-Member -NotePropertyName $k -NotePropertyValue $mode.Settings[$k] -Force }
        [System.IO.File]::WriteAllText($settingsPath, ($s | ConvertTo-Json -Depth 20))

        $argList = @($mode.Args | Where-Object { $_ })
        $p = if ($argList) { Start-Process $Exe -ArgumentList $argList -PassThru } else { Start-Process $Exe -PassThru }
        Start-Sleep -Seconds $Wait

        $alive = -not $p.HasExited
        $wins = if ($alive) { [SmokeWin]::Of([uint32]$p.Id) } else { @() }
        $r = [pscustomobject]@{
            Alive       = $alive
            Windows     = $wins
            Main        = ($wins | Where-Object { $_.Title -eq 'RigStats' } | Select-Object -First 1)
            HostRunning = [bool](Get-Process rigstats-wallpaper -ErrorAction SilentlyContinue)
            Log         = (Get-Content $logPath -Raw -ErrorAction SilentlyContinue) ?? ''
        }
        if (-not $r.Main) { $r.Main = [pscustomobject]@{ Visible = $false; X = 0 } }

        $bad = @()
        foreach ($c in @($common.GetEnumerator()) + @($mode.Checks.GetEnumerator())) {
            if (-not (& $c.Value)) { $bad += $c.Key }
        }
        if ($bad) {
            $failed += $name
            Write-Host ("FAIL  {0,-13} {1}" -f $name, ($bad -join '; ')) -ForegroundColor Red
            $r.Log -split "`r?`n" | Where-Object { $_ -match '\[ERROR\]|panicked' } | ForEach-Object { Write-Host "      $_" -ForegroundColor DarkGray }
            $wins | ForEach-Object { Write-Host ("      window '{0}' visible={1} at {2},{3} {4}x{5}" -f $_.Title, $_.Visible, $_.X, $_.Y, $_.W, $_.H) -ForegroundColor DarkGray }
        } else {
            Write-Host ("PASS  {0}" -f $name) -ForegroundColor Green
        }
    }
} finally {
    Stop-RigStats
    [System.IO.File]::WriteAllBytes($settingsPath, $original)
    Write-Host 'Settings restored.'
    if ($Restart) {
        $installed = Join-Path ${env:ProgramFiles} 'RIGStats\rigstats.exe'
        if (Test-Path $installed) { Start-Process $installed; Write-Host 'Installed RIGStats started.' }
    }
}
Write-Host ("{0} of {1} modes passed." -f ($names.Count - $failed.Count), $names.Count)
exit $failed.Count
