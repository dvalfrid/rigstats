# Measures what RIGStats costs the PC it runs on (#463, part of #462).
# Read-only: it only reads Windows' performance counters — no WMI while
# measuring (WmiPrvSE is one of the processes measured), nothing changed.
#
#   pwsh -File tools\measure-resources.ps1 -Minutes 30 -Label release-normal
#
# Every -Interval seconds it samples each rigstats* process and WmiPrvSE:
# CPU (% of one core, averaged over the whole interval by Windows), working
# set, private bytes, threads, handles — and for the app (rigstats.exe) its
# GPU dedicated memory and GPU engine utilization. Writes one CSV row per
# process per sample to -Out and prints a summary per process at the end:
# average and peak CPU, min/max working set, private bytes and GPU memory,
# and handle drift (last minus first). The sensor service's three processes
# are told apart by their command line when Windows lets this shell read it
# (else by pid).
param(
    [double]$Minutes = 10,
    [int]$Interval = 10,
    [string]$Label = 'run',
    [string]$Out = (Join-Path $env:TEMP "rigstats-resources-$(Get-Date -Format yyyyMMdd-HHmmss).csv")
)

$cores = [Environment]::ProcessorCount
$names = @('rigstats', 'rigstats-wallpaper', 'rigstats-sensor', 'wmiprvse')

# Roles of the service's processes, read once (CIM is WMI — not in the loop).
$roles = @{}
try {
    Get-CimInstance Win32_Process -Filter "Name LIKE 'rigstats%'" -ErrorAction Stop | ForEach-Object {
        $role = switch -Regex ($_.CommandLine) {
            '--hardware-worker' { 'sensor-hardware-worker' }
            '--gpu-worker' { 'sensor-gpu-worker' }
            'rigstats-sensor' { 'sensor-service' }
            default { $null }
        }
        if ($role) { $roles[[int]$_.ProcessId] = $role }
    }
} catch { }

function Name-Of([string]$proc, [int]$id) {
    if ($proc -eq 'rigstats-sensor') { if ($roles.ContainsKey($id)) { return $roles[$id] } return "sensor-$id" }
    if ($proc -eq 'wmiprvse') { return "WmiPrvSE-$id" }
    return $proc
}

# "\\pc\process(rigstats-sensor#1)\working set" -> ("rigstats-sensor#1", "working set").
# The instance comes from the path: InstanceName drops the "#1" that tells
# same-named processes apart.
function Split-Path-Of([string]$path) {
    $open = $path.IndexOf('(') + 1
    $close = $path.LastIndexOf(')')
    [pscustomobject]@{ Instance = $path.Substring($open, $close - $open); Field = $path.Substring($path.LastIndexOf('\') + 1) }
}

$paths = @(
    '\Process(*)\ID Process', '\Process(*)\% Processor Time', '\Process(*)\Working Set',
    '\Process(*)\Private Bytes', '\Process(*)\Thread Count', '\Process(*)\Handle Count',
    '\GPU Process Memory(*)\Dedicated Usage', '\GPU Engine(*)\Utilization Percentage'
)
$count = [math]::Max(1, [int][math]::Round($Minutes * 60 / $Interval))
Write-Host "Measuring $count samples every $Interval s ($([math]::Round($count * $Interval / 60, 1)) min, $cores cores) -> $Out"

$rows = [System.Collections.Generic.List[object]]::new()
# One continuous read: each sample's rates are averaged over the interval.
Get-Counter -Counter $paths -SampleInterval $Interval -MaxSamples $count -ErrorAction SilentlyContinue | ForEach-Object {
    $at = $_.Timestamp.ToString('HH:mm:ss')
    $procs = @{}
    $gpuMem = @{}
    $gpuUtil = @{}
    foreach ($s in $_.CounterSamples) {
        $p = Split-Path-Of $s.Path
        if ($s.Path -match '\\gpu process memory\(pid_(\d+)_') { $gpuMem[[int]$Matches[1]] += $s.CookedValue; continue }
        if ($s.Path -match '\\gpu engine\(pid_(\d+)_') { $gpuUtil[[int]$Matches[1]] += $s.CookedValue; continue }
        $proc = ($p.Instance -replace '#\d+$', '').ToLowerInvariant()
        if ($names -notcontains $proc) { continue }
        if (-not $procs.ContainsKey($p.Instance)) { $procs[$p.Instance] = @{ proc = $proc } }
        $procs[$p.Instance][$p.Field] = $s.CookedValue
    }
    foreach ($v in $procs.Values) {
        $id = [int]$v['id process']
        # A process that came or went mid-sample has only some of its fields.
        if (-not $id -or -not $v['working set']) { continue }
        $app = $v.proc -eq 'rigstats'
        $rows.Add([pscustomobject]@{
            Time = $at; Label = $Label; Process = Name-Of $v.proc $id; Pid = $id
            # % Processor Time adds up the logical CPUs: 100 = one core.
            CpuPctOneCore = [math]::Round($v['% processor time'], 2)
            WorkingSetMB = [math]::Round($v['working set'] / 1MB, 1)
            PrivateMB = [math]::Round($v['private bytes'] / 1MB, 1)
            Threads = [int]$v['thread count']
            Handles = [int]$v['handle count']
            GpuDedicatedMB = if ($app) { [math]::Round(($gpuMem[$id] ?? 0) / 1MB, 1) } else { $null }
            GpuUtilPct = if ($app) { [math]::Round(($gpuUtil[$id] ?? 0), 2) } else { $null }
        })
    }
}

$rows | Export-Csv $Out -NoTypeInformation -Encoding utf8
Write-Host ""
Write-Host "Summary ($Label, $($rows.Count) rows):"
$rows | Group-Object Process | ForEach-Object {
    $g = $_.Group
    $cpu = $g | Measure-Object CpuPctOneCore -Average -Maximum
    $ws = $g | Measure-Object WorkingSetMB -Minimum -Maximum
    $priv = $g | Measure-Object PrivateMB -Minimum -Maximum
    $gpu = $g | Where-Object { $null -ne $_.GpuDedicatedMB } | Measure-Object GpuDedicatedMB -Minimum -Maximum
    [pscustomobject]@{
        Process = $_.Name
        'CPU avg %' = [math]::Round($cpu.Average, 2)
        'CPU max %' = [math]::Round($cpu.Maximum, 1)
        'WS MB' = "$($ws.Minimum)-$($ws.Maximum)"
        'Private MB' = "$($priv.Minimum)-$($priv.Maximum)"
        'GPU MB' = if ($gpu.Count) { "$($gpu.Minimum)-$($gpu.Maximum)" } else { '' }
        Threads = ($g | Select-Object -Last 1).Threads
        Handles = ($g | Select-Object -Last 1).Handles
        'Handle drift' = ($g | Select-Object -Last 1).Handles - ($g | Select-Object -First 1).Handles
    }
} | Sort-Object Process | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
Write-Host "CSV: $Out"
