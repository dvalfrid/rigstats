using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using SensorSidecar;
using SensorSidecar.Control;
using SensorSidecar.Control.Lighting;
using SensorSidecar.Control.Providers;

var dryRun = args.Contains("--dry-run");

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

// Control Center phase 3 (#190): GPU power limit — AMD via ADLX (the
// Adrenalin driver's own SDK); NVIDIA (NVML) is #210.
builder.Services.AddSingleton<IControlProvider>(_ => new GpuPowerProvider(
    AdlxGpuPower.TryLoad(),
    dryRun,
    Path.Combine(programData, "gpu-power-original.json")));

// Control Center phase 5 (#192): lighting — ASUS Aura USB controllers. Also
// registered as itself for the control pipe's live preview, like FanProvider.
builder.Services.AddSingleton(_ => new LightingProvider(
    Hid.Enumerate,
    hid =>
    {
        // Every lighting device found; looked for again when devices change.
        var devices = new List<ILightingDevice>();
        if (AuraController.TryOpen(out var reason) is { } motherboard)
            devices.Add(motherboard);
        // ASUS Aura monitors and the monitor light bar (#212).
        devices.AddRange(AsusMonitorDevice.Discover(hid));
        // ASUS TUF-protocol keyboards — the ROG Azoth X via the Omni receiver or cable (#213).
        devices.AddRange(AsusKeyboardDevice.Discover(hid));
        // ASUS headsets on the GearLink protocol — the ROG Delta II via its dongle.
        devices.AddRange(AsusHeadsetDevice.Discover(hid));
        // Any HID LampArray device (Windows Dynamic Lighting standard), any brand.
        devices.AddRange(LampArrayDevice.Discover(hid, LampArrayDevice.WindowsDynamicLightingOn));
        return new LightingScan(devices, devices.Count > 0 ? "" : reason);
    },
    LightingProvider.DetectConflict,
    dryRun,
    // For the diagnostics export: what was found, and every HID collection
    // seen, so unknown devices can be supported from a user's export.
    (hid, devices, reason) => LightingDiagnostics.Write(
        Path.Combine(programData, "lighting-devices.json"),
        LightingDiagnostics.Build(hid, devices, reason, LightingProvider.DetectConflict(), LampArrayDevice.WindowsDynamicLightingOn()))));
builder.Services.AddSingleton<IControlProvider>(sp => sp.GetRequiredService<LightingProvider>());

// Last: every provider exists and the hardware is open by the time the
// stored active profile is re-applied.
builder.Services.AddHostedService<ActiveProfileApplier>();

var host = builder.Build();

// Doc: "Release on stop | StopAsync and a top-level finally call
// ReleaseToFirmware() on all providers." StopAsync is covered by
// SafetyGuard's own IHostedService registration above; this finally is the
// backstop for an unhandled exception escaping host.Run() itself. Resolved
// up front: Run() disposes the host's service provider on the way out.
var safetyGuard = host.Services.GetRequiredService<SafetyGuard>();
try
{
    host.Run();
}
finally
{
    safetyGuard.ReleaseAllToFirmware();
}
