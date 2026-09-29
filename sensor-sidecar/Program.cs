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

// Control Center phase 0 (#187): one provider (power plan), the broker that
// transacts profile applies across whatever providers are registered, the
// service-owned profile store, and the duplex control pipe.
builder.Services.AddSingleton<IPowerPlanApi, Win32PowerPlanApi>();
builder.Services.AddSingleton<IControlProvider, PowerPlanProvider>();
builder.Services.AddSingleton<ControlBroker>();
builder.Services.AddSingleton<ProfileStore>();
builder.Services.AddSingleton<IPipeClientVerifier, PipeClientVerifier>();
builder.Services.AddSingleton(sp => new SafetyGuard(sp.GetServices<IControlProvider>(), dryRun));
builder.Services.AddHostedService(sp => sp.GetRequiredService<SafetyGuard>());
builder.Services.AddHostedService<ControlPipeWorker>();

var host = builder.Build();

// Doc: "Release on stop | StopAsync and a top-level finally call
// ReleaseToFirmware() on all providers." StopAsync is covered by
// SafetyGuard's own IHostedService registration above; this finally is the
// backstop for an unhandled exception escaping host.Run() itself.
try
{
    host.Run();
}
finally
{
    host.Services.GetRequiredService<SafetyGuard>().ReleaseAllToFirmware();
}
