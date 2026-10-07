using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using SensorSidecar;
using SensorSidecar.Control;
using SensorSidecar.Control.Lighting;
using SensorSidecar.Control.Providers;

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

// Earlier crashes Windows recorded but this log couldn't (a native fault ends
// the process before any handler runs, #242) — copied in once.
if (SidecarLog.FileEnabled)
    LogEarlierCrashes(Path.Combine(DataDirectory.DefaultPath, "crash-report-mark.txt"));

var builder = Host.CreateApplicationBuilder(args);
builder.Services.AddWindowsService(options =>
{
    options.ServiceName = "RIGStats Sensor";
});

// HardwareHost owns the single LHM Computer instance and its lock — resolved
// as IHardwareHost by SensorWorker/providers, and separately started/stopped
// via its own IHostedService registration (must run before SensorWorker, so
// register it first: hosted services start in registration order).
builder.Services.AddSingleton<HardwareHost>();
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
builder.Services.AddSingleton(sp => new SafetyGuard(sp.GetServices<IControlProvider>(), dryRun));
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
    Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        "se.codeby.rigstats",
        "fan-channels.json")));
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
var programData = Path.Combine(
    Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
    "se.codeby.rigstats");
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
// Adrenalin driver's own SDK), NVIDIA desktop GPUs via NVML.
builder.Services.AddSingleton<IControlProvider>(_ =>
{
    var adlx = AdlxGpuPower.TryLoad();
    var nvml = NvmlGpuPower.TryLoad();
    // For the diagnostics export: what NVML reports from the service. ADLX
    // isn't opened for it (#241); its adapters are in the capabilities.
    LightingDiagnostics.Write(Path.Combine(programData, "gpu-power.json"), new System.Text.Json.Nodes.JsonObject
    {
        ["written_utc"] = DateTimeOffset.UtcNow.ToString("O"),
        ["adlx_installed"] = adlx is not null,
        ["nvml"] = nvml?.Diagnostics(),
    });
    return new GpuPowerProvider(GpuPowerApis.Of(adlx, nvml), dryRun, Path.Combine(programData, "gpu-power-original.json"));
});

// The paired Hue Bridge (#215): its chosen rooms and zones are lighting
// devices like any other; the control pipe pairs it.
builder.Services.AddSingleton(_ => new HueLink(Path.Combine(programData, "hue.json")));

// Control Center phase 5 (#192): lighting — ASUS Aura USB controllers. Also
// registered as itself for the control pipe's live preview, like FanProvider.
builder.Services.AddSingleton(sp => new LightingProvider(
    Hid.Enumerate,
    hid =>
    {
        // Every lighting device found; looked for again when devices change.
        var devices = new List<ILightingDevice>();
        if (AuraController.TryOpen(out var reason) is { } motherboard)
            devices.Add(motherboard);
        // ASUS Aura monitors and the monitor light bar (#212).
        devices.AddRange(AsusMonitorDevice.Discover(hid));
        // ASUS TUF-protocol keyboards — the ROG Azoth X via the Omni receiver
        // or cable, the ROG Falchion Ace HFX (#213).
        var keyboards = AsusKeyboardDevice.Discover(hid);
        devices.AddRange(keyboards);
        // ASUS headsets on the GearLink protocol — the ROG Delta II via its
        // dongle. A headset switched off is asked again on later rescans.
        var silent = new List<HidDeviceInfo>();
        devices.AddRange(AsusHeadsetDevice.Discover(hid, silent));
        // Any HID LampArray device (Windows Dynamic Lighting standard), any
        // brand — except keyboards already driven by their own protocol above.
        devices.AddRange(LampArrayDevice.Discover(
            AsusKeyboardDevice.WithoutDirectKeyboards(hid, keyboards), LampArrayDevice.WindowsDynamicLightingOn));
        // Philips Hue rooms and zones chosen to follow the rig — over the
        // network, from the paired bridge.
        devices.AddRange(sp.GetRequiredService<HueLink>().Devices());
        return new LightingScan(devices, devices.Count > 0 ? "" : reason, RetryHeadsets(silent));
    },
    LightingProvider.DetectConflict,
    dryRun,
    // For the diagnostics export: what was found, and every HID collection
    // seen, so unknown devices can be supported from a user's export.
    (hid, devices, reason) => LightingDiagnostics.Write(
        Path.Combine(programData, "lighting-devices.json"),
        LightingDiagnostics.Build(hid, devices, reason, LightingProvider.DetectConflict(), LampArrayDevice.WindowsDynamicLightingOn(),
            ProbeAsus(hid))),
    hue: sp.GetRequiredService<HueLink>()));
builder.Services.AddSingleton<IControlProvider>(sp => sp.GetRequiredService<LightingProvider>());

// Last: every provider exists and the hardware is open by the time the
// stored active profile is re-applied.
builder.Services.AddHostedService<ActiveProfileApplier>();

IHost host;
SafetyGuard safetyGuard;
try
{
    host = builder.Build();
    // Doc: "Release on stop | StopAsync and a top-level finally call
    // ReleaseToFirmware() on all providers." StopAsync is covered by
    // SafetyGuard's own IHostedService registration above; the finally below
    // is the backstop for an unhandled exception escaping host.Run() itself.
    // Resolved up front: Run() disposes the host's service provider on the
    // way out.
    safetyGuard = host.Services.GetRequiredService<SafetyGuard>();
}
catch (Exception e)
{
    // A provider that can't even be constructed: say which, then fail the
    // start (the service manager retries).
    SidecarLog.Log($"[rigstats-sensor] Service failed to start: {e}");
    throw;
}
try
{
    host.Run();
}
catch (Exception e)
{
    // E.g. the hardware (LHM/PawnIO) not opening in a hosted service's start.
    SidecarLog.Log($"[rigstats-sensor] Service stopped by an error: {e}");
    throw;
}
finally
{
    safetyGuard.ReleaseAllToFirmware();
}

// Reads ".NET Runtime" 1026 / "Application Error" 1000 entries since the last
// one logged and writes this service's into its log; never fails the start.
static void LogEarlierCrashes(string markPath)
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
        var lines = CrashReport.Lines(events, since);
        foreach (var line in lines)
            SidecarLog.Log(line);
        if (lines.Count > 0)
            CrashReport.WriteMark(markPath, events.Where(e => e.Time > since).Max(e => e.Time));
    }
    catch (Exception e)
    {
        SidecarLog.Log($"[rigstats-sensor] Earlier crashes not read from the event log: {e.Message}");
    }
}

// The ASUS receivers' read-only replies for the diagnostics file; never
// fails the discovery it is written after.
static System.Text.Json.Nodes.JsonArray? ProbeAsus(IReadOnlyList<HidDeviceInfo> hid)
{
    try
    {
        return OmniMouse.Probe(hid);
    }
    catch (Exception e)
    {
        SidecarLog.Log($"[rigstats-control] Lighting: ASUS receiver probe failed: {e.Message}");
        return null;
    }
}

// Asks the silent headset dongles again, until every headset has answered.
static Func<LightingScan>? RetryHeadsets(List<HidDeviceInfo> silent) => silent.Count == 0 ? null : () =>
{
    var stillSilent = new List<HidDeviceInfo>();
    var found = AsusHeadsetDevice.Discover(silent, stillSilent);
    return new LightingScan(found, "", RetryHeadsets(stillSilent));
};
