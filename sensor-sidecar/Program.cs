using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using SensorSidecar;
using SensorSidecar.Control;
using SensorSidecar.Control.Lighting;
using SensorSidecar.Control.Providers;
using SensorSidecar.Control.Workers;

// A crash anywhere — a worker thread, a timer — is logged before the process
// ends. The service manager restarts the service; without this the reason
// would only be in the Windows event log (#219).
AppDomain.CurrentDomain.UnhandledException += (_, e) =>
    SidecarLog.Log($"[rigstats-sensor] Fatal unhandled exception{(e.IsTerminating ? " — the service stops" : "")}: {e.ExceptionObject}");
TaskScheduler.UnobservedTaskException += (_, e) =>
{
    SidecarLog.Log($"[rigstats-sensor] Unobserved task exception: {e.Exception}");
    e.SetObserved();
};

// The GPU driver libraries run in a child process of the service (#370): a
// native fault in a vendor DLL then ends the child, not the service. The
// child logs on stderr, which the service writes to its log.
if (args.Contains("--gpu-worker"))
{
    SidecarLog.FileEnabled = false;
    GpuWorkerHost.Run(args, DataDirectory.DefaultPath);
    return;
}

// LibreHardwareMonitor, with the vendor libraries it loads for the GPU
// sensors, runs in a child process too (#374): a native fault there ends the
// child, which the service starts again.
if (args.Contains("--hardware-worker"))
{
    SidecarLog.FileEnabled = false;
    HardwareWorkerHost.Run(args);
    return;
}

var dryRun = args.Contains("--dry-run");

// Before anything reads or writes the service's state: the data folder is
// locked down to SYSTEM and Administrators. If that fails, what is in it
// can't be trusted — nothing is written to hardware and nothing to the log.
if (DataDirectory.EnsureSecure(DataDirectory.DefaultPath) is { } insecure)
{
    SidecarLog.FileEnabled = false;
    dryRun = true;
    SidecarLog.Log($"[rigstats-sensor] The data folder could not be secured ({insecure}) — running without hardware control.");
}

// The crash dump folder (#220) is locked to administrators before Windows
// ever writes into it.
if (SidecarLog.FileEnabled && DataDirectory.EnsureDumpFolder(DataDirectory.DumpPath) is { } dumpFolderProblem)
    SidecarLog.Log($"[rigstats-sensor] The crash dump folder could not be secured ({dumpFolderProblem}).");

// Nothing slow may run before host.Run() (#383): the service manager gives a
// starting service 30 s to connect, and it connects inside Run(), before the
// hosted services below are built. So the hardware is opened, the lighting
// devices discovered and the event log read only after that.

var builder = Host.CreateApplicationBuilder(args);
builder.Services.AddWindowsService(options =>
{
    options.ServiceName = "RIGStats Sensor";
});

// One-off start-up work in the background once the service has started
// (#383): which other hardware software runs here (Armoury Crate's services,
// OpenRGB — the evidence, in users' diagnostics, for what owns the fans and
// the lighting on boards we don't have, #193), then earlier crashes Windows
// recorded but this log couldn't (#242). One task for both: AddHostedService
// drops a second registration of the same type (TryAddEnumerable), so a
// second BackgroundStartupTask would never run.
if (SidecarLog.FileEnabled)
{
    builder.Services.AddHostedService(_ => new BackgroundStartupTask("Start-up checks", () =>
    {
        SidecarLog.Log($"[rigstats-control] {OtherSoftware.Summary()}");
        LogEarlierCrashes(Path.Combine(DataDirectory.DefaultPath, "crash-report-mark.txt"), DataDirectory.DumpPath);
    }));
}

// The host's own warnings and errors (a worker that failed, a slow stop) in
// rigstats-sensor.log too, not only the Windows event log (#378).
builder.Logging.AddProvider(new SidecarLoggerProvider());

// HardwareHost is the one way to the hardware LibreHardwareMonitor reads and
// writes — in the hardware worker process (#374) — with the shared sample;
// resolved as IHardwareHost by SensorWorker/providers, and separately
// stopped via its own IHostedService registration (registered first: hosted
// services stop in reverse order, so the worker ends last).
var hardwareWorkerArgs = "--hardware-worker";
#if DEBUG
// Live tests: prove the service survives a crashing or hanging worker.
foreach (var test in new[] { "--crash-after", "--hang-after", "--crash-reading" })
{
    var at = Array.IndexOf(args, "--hardware-worker" + test[1..]);
    if (at >= 0 && at + 1 < args.Length)
        hardwareWorkerArgs += $" {test} {args[at + 1]}";
}
#endif
builder.Services.AddSingleton<PeripheralStatusStore>();
builder.Services.AddSingleton(sp => new HardwareHost(
    new HardwareWorkerClient(skip => WorkerProcess.Start(Environment.ProcessPath!,
        skip.Count == 0 ? hardwareWorkerArgs : $"{hardwareWorkerArgs} {HardwareWorkerProtocol.SkipArgument} {string.Join(',', skip)}", "hardware-worker")),
    sp.GetRequiredService<PeripheralStatusStore>(), sp.GetRequiredService<ProfileStore>()));
builder.Services.AddSingleton<IHardwareHost>(sp => sp.GetRequiredService<HardwareHost>());
builder.Services.AddHostedService(sp => sp.GetRequiredService<HardwareHost>());

builder.Services.AddHostedService<SensorWorker>();

// Control Center phase 0 (#187): the broker that transacts profile applies
// across whatever providers are registered, the service-owned profile
// store, and the duplex control pipe.
builder.Services.AddSingleton<IPowerPlanApi, Win32PowerPlanApi>();
builder.Services.AddSingleton<IControlProvider, PowerPlanProvider>();
builder.Services.AddSingleton<ControlBroker>();
builder.Services.AddSingleton<ProfileStore>();
builder.Services.AddSingleton<IPipeClientVerifier, PipeClientVerifier>();
// Kept for the top-level finally below, once the host has built it — never
// resolved before Run() (#383): building it builds every provider.
SafetyGuard? safetyGuard = null;
builder.Services.AddSingleton(sp => safetyGuard = new SafetyGuard(sp.GetServices<IControlProvider>(), dryRun));
builder.Services.AddHostedService(sp => sp.GetRequiredService<SafetyGuard>());
builder.Services.AddHostedService<ControlPipeWorker>();

// Control Center phase 1 (#188): fan curves, evaluated service-side at ~1Hz.
// FanProvider is registered as itself too (not just IControlProvider) so
// FanCurveLoop can reach GetActiveConfig()/ApplyDuty() — neither is part of
// the IControlProvider contract — mirroring HardwareHost's own dual
// registration above. FanCurveLoop likewise resolves to one instance, so
// the control pipe can read its event stream. Registered after SafetyGuard:
// hosted services stop in reverse order, so the loop is stopped before
// SafetyGuard hands every header back to the BIOS.
builder.Services.AddSingleton(sp => new FanProvider(
    sp.GetRequiredService<IHardwareHost>(),
    dryRun,
    Path.Combine(DataDirectory.DefaultPath, "fan-channels.json"),
    OtherSoftware.FansTaken));
builder.Services.AddSingleton<IControlProvider>(sp => sp.GetRequiredService<FanProvider>());
builder.Services.AddSingleton<FanCurveLoop>();
builder.Services.AddHostedService(sp => sp.GetRequiredService<FanCurveLoop>());
// Restarts the service if the loop above stops ticking (#222). After it, so
// it stops first; not in dry-run, where nothing is written to the fans.
if (!dryRun)
    builder.Services.AddHostedService<FanWatchdog>();

// Control Center phase 2 (#189): CPU package limits through the SMU. The
// boot-crash guard reads (and consumes) the previous run's marker when it is
// constructed — before ActiveProfileApplier decides what to re-apply.
var programData = DataDirectory.DefaultPath;
builder.Services.AddSingleton(_ => new BootCrashGuard(
    Path.Combine(programData, "pending-apply"),
    BootCrashGuard.DefaultStableAfter));
// One SMU handle for CPU limits and Curve Optimizer (#191).
var smu = new Lazy<(RyzenSmu? Smu, string Reason)>(() =>
{
    var opened = RyzenSmu.TryOpen(out var reason);
    return (opened, reason);
});
builder.Services.AddSingleton<IControlProvider>(sp => new CpuLimitProvider(
    smu.Value.Smu,
    smu.Value.Reason,
    sp.GetRequiredService<BootCrashGuard>(),
    dryRun,
    Path.Combine(programData, "cpu-limit-baseline.json"),
    CpuLimitProvider.CurrentBootTime()));

// Control Center phase 4 (#191): Curve Optimizer through the same SMU, only
// on generations whose CO commands are verified.
builder.Services.AddSingleton<IControlProvider>(sp => new CurveOptimizerProvider(
    smu.Value.Smu?.CurveOptimizer,
    CurveOptimizerProvider.WindowsPhysicalCores(),
    smu.Value.Smu is null ? smu.Value.Reason : "Curve Optimizer is not supported on this CPU yet.",
    sp.GetRequiredService<BootCrashGuard>(),
    dryRun,
    Path.Combine(programData, "curve-opt-baseline.json"),
    CpuLimitProvider.CurrentBootTime()));

// Control Center phase 3 (#190, #210): GPU power limit — AMD via ADLX (the
// Adrenalin driver's own SDK), NVIDIA desktop GPUs via NVML. Both run in the
// GPU worker process (#370); the worker writes gpu-power.json.
builder.Services.AddSingleton<IControlProvider>(_ =>
{
    if (!GpuCrashGuard.AnyDriverInstalled())
        return new GpuPowerProvider(null, dryRun, Path.Combine(programData, "gpu-power-original.json"));
    var workerArgs = "--gpu-worker";
#if DEBUG
    // Live tests: prove the service survives a crashing or hanging driver.
    if (args.Contains("--gpu-worker-crash-test"))
        workerArgs += " --crash-test";
    else if (args.Contains("--gpu-worker-hang-test"))
        workerArgs += " --hang-test";
#endif
    var worker = new GpuWorkerClient(
        () => WorkerProcess.Start(Environment.ProcessPath!, workerArgs, "gpu-worker"),
        Path.Combine(programData, "gpu-worker-crash.marker"),
        CpuLimitProvider.CurrentBootTime(),
        GpuCrashGuard.DriverFiles());
    return new GpuPowerProvider(worker, dryRun, Path.Combine(programData, "gpu-power-original.json"));
});

// The paired Hue Bridge (#215): its chosen rooms and zones are lighting
// devices like any other; the control pipe pairs it.
builder.Services.AddSingleton(_ => new HueLink(Path.Combine(programData, "hue.json")));

// Control Center phase 5 (#192): lighting — every device LightingDiscovery
// finds. Also registered as itself for the control pipe's live preview, like
// FanProvider.
builder.Services.AddSingleton(sp => new LightingProvider(
    Hid.Enumerate,
    hid => LightingDiscovery.Scan(hid, sp.GetRequiredService<HueLink>()),
    OtherSoftware.LightingTaken,
    dryRun,
    (hid, devices, reason) => LightingDiscovery.WriteDiagnostics(
        Path.Combine(programData, "lighting-devices.json"), hid, devices, reason),
    hue: sp.GetRequiredService<HueLink>()));
builder.Services.AddSingleton<IControlProvider>(sp => sp.GetRequiredService<LightingProvider>());

// Last: every provider exists and the hardware is open by the time the
// stored active profile is re-applied.
builder.Services.AddHostedService<ActiveProfileApplier>();
// Batteries of the wireless lighting devices, about once a minute (#290).
builder.Services.AddHostedService<PeripheralBatteryMonitor>();

IHost host;
try
{
    // Builds nothing yet: services are created when Run() starts them.
    host = builder.Build();
}
catch (Exception e)
{
    SidecarLog.Log($"[rigstats-sensor] Service failed to start: {e}");
    throw;
}
try
{
    host.Run();
}
catch (Exception e)
{
    // E.g. a provider that can't be constructed, or the hardware (LHM/PawnIO)
    // not opening in a hosted service's start.
    SidecarLog.Log($"[rigstats-sensor] Service stopped by an error: {e}");
    throw;
}
finally
{
    // Doc: "Release on stop | StopAsync and a top-level finally call
    // ReleaseToFirmware() on all providers." StopAsync is covered by
    // SafetyGuard's own hosted-service registration; this is the backstop
    // for an exception escaping Run() itself. Null when the host never got
    // as far as building it — then no provider was built either.
    safetyGuard?.ReleaseAllToFirmware();
}

// Reads ".NET Runtime" 1026 / "Application Error" 1000 entries since the last
// one logged and writes this service's into its log. Here rather than in
// Control/: the event log API is only referenced by the service itself.
static void LogEarlierCrashes(string markPath, string dumpFolder)
{
    try
    {
        var now = DateTimeOffset.Now;
        var since = CrashReport.ReadMark(markPath) ?? now - CrashReport.FirstLookBack;
        var window = (long)Math.Clamp((now - since).TotalMilliseconds, 0, TimeSpan.FromDays(30).TotalMilliseconds);
        var query = new System.Diagnostics.Eventing.Reader.EventLogQuery("Application",
            System.Diagnostics.Eventing.Reader.PathType.LogName,
            "*[System[Provider[@Name='.NET Runtime' or @Name='Application Error']"
            + $" and (EventID={CrashReport.DotNetRuntimeId} or EventID={CrashReport.ApplicationErrorId})"
            + $" and TimeCreated[timediff(@SystemTime) <= {window}]]]");
        var events = new List<CrashEvent>();
        using (var reader = new System.Diagnostics.Eventing.Reader.EventLogReader(query))
        {
            while (reader.ReadEvent() is { } record)
            {
                using (record)
                {
                    if (record.TimeCreated is { } time && record.FormatDescription() is { } message)
                        events.Add(new CrashEvent(new DateTimeOffset(time), record.Id, message));
                }
            }
        }
        var dumps = Directory.Exists(dumpFolder)
            ? new DirectoryInfo(dumpFolder).EnumerateFiles("*.dmp")
                .Select(f => new CrashDump(f.Name, new DateTimeOffset(f.LastWriteTime), f.Length))
                .ToList()
            : [];
        var lines = CrashReport.Lines(events, since).Concat(CrashReport.DumpLines(dumps, since, dumpFolder)).ToList();
        foreach (var line in lines)
            SidecarLog.Log(line);
        // Past every entry and dump just logged: a dump is written moments
        // after its event, and must not be logged again at the next start.
        if (lines.Count > 0)
        {
            CrashReport.WriteMark(markPath, events.Select(e => e.Time).Concat(dumps.Select(d => d.Time))
                .Where(t => t > since).Max());
        }
    }
    catch (Exception e)
    {
        SidecarLog.Log($"[rigstats-sensor] Earlier crashes not read from the event log: {e.Message}");
    }
}
