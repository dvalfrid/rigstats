# CPU of the app over time after it starts (#663): tells AMD's start-up burst
# (#515: a driver thread takes a whole core for ~30–120 s after a process
# starts rendering) from a steady cost. A 20 s window right after start only
# ever measures the burst.
#
#   pwsh -File tools\measure-startup-cpu.ps1 -Exe "$env:ProgramFiles\RIGStats\rigstats.exe"
#   pwsh -File tools\measure-startup-cpu.ps1 -Exe .\target\release\rigstats.exe -Layer normal
#   pwsh -File tools\measure-startup-cpu.ps1 -Exe .\target\release\rigstats.exe -Floating -At 15,60,120
#
# Stops every running rigstats.exe / rigstats-wallpaper.exe first (the
# installed app included), sets the window layer (default: the desktop
# wallpaper), floating panels and no overlay, starts -Exe, and prints the CPU
# of rigstats and rigstats-wallpaper (% of one core) over a 20 s window
# starting at each of -At seconds after start. Then stops them and puts your
# settings back; a copy stays next to them (.measure-backup) if that fails.
# See tools/README.md.
param(
    [Parameter(Mandatory)][string]$Exe,
    [string]$Layer = 'wallpaper',
    [switch]$Floating,
    [int[]]$At = @(15, 60, 120, 180, 240),
    [int]$Window = 20
)

$settingsPath = Join-Path $env:APPDATA 'se.codeby.rigstats\rigstats-settings.json'
$orig = [IO.File]::ReadAllBytes($settingsPath)
$backup = "$settingsPath.measure-backup"
[IO.File]::WriteAllBytes($backup, $orig)

function Stop-RigStats {
    Get-Process rigstats, rigstats-wallpaper -ErrorAction SilentlyContinue |
        ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }
}

try {
    Stop-RigStats
    Start-Sleep 2
    $s = [Text.Encoding]::UTF8.GetString($orig) | ConvertFrom-Json
    $s | Add-Member windowLayer $Layer -Force
    $s | Add-Member floatingMode ([bool]$Floating) -Force
    $s | Add-Member overlayEnabled $false -Force
    [IO.File]::WriteAllText($settingsPath, ($s | ConvertTo-Json -Depth 20))

    "$Exe — layer $Layer, floating $([bool]$Floating)"
    $start = Get-Date
    Start-Process $Exe | Out-Null
    foreach ($t in $At) {
        $wait = $t - ((Get-Date) - $start).TotalSeconds
        if ($wait -gt 0) { Start-Sleep -Seconds $wait }
        $procs = @(Get-Process rigstats, rigstats-wallpaper -ErrorAction SilentlyContinue)
        $t0 = @{}
        $procs | ForEach-Object { $t0[$_.Id] = $_.TotalProcessorTime }
        Start-Sleep -Seconds $Window
        $cpu = $procs | ForEach-Object {
            $_.Refresh()
            '{0} {1:N1} %' -f $_.ProcessName, (($_.TotalProcessorTime - $t0[$_.Id]).TotalMilliseconds / ($Window * 10))
        }
        '{0,4}-{1,-4}s: {2}' -f $t, ($t + $Window), ($cpu -join ', ')
    }
} finally {
    Stop-RigStats
    [IO.File]::WriteAllBytes($settingsPath, $orig)
    if ((Get-FileHash $settingsPath).Hash -eq (Get-FileHash $backup).Hash) {
        Remove-Item $backup
    } else {
        Write-Warning "Settings not restored — copy $backup back to $settingsPath"
    }
}
