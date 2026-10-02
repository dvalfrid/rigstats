using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using SensorSidecar;
using SensorSidecar.Control;
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
