using Microsoft.Extensions.Hosting;

namespace SensorSidecar.Control;

/// Doc ("Profile model"): "The service re-applies the active profile at
/// start-up, so profiles work before anyone logs in." Without this, fan
/// curves would be gone after every reboot or service restart until someone
/// re-applied the profile from the UI. Runs once, through the same broker
/// transaction as a UI apply. When risky domains arrive (CPU limits #189,
/// Curve Optimizer #191) the boot-crash guard must gate them here.
public sealed class ActiveProfileApplier(ProfileStore profiles, ControlBroker broker) : IHostedService
{
    public async Task StartAsync(CancellationToken cancellationToken)
    {
        try
        {
            var id = await profiles.GetActiveIdAsync(cancellationToken);
            if (id is null || await profiles.GetAsync(id, cancellationToken) is not { } profile)
                return;
            var result = await broker.ApplyProfileAsync(profile, cancellationToken);
            SidecarLog.Log(result.Ok
                ? $"[rigstats-control] Re-applied active profile '{id}' at start-up."
                : $"[rigstats-control] Active profile '{id}' not re-applied at start-up: {result.Message}");
        }
        catch (Exception e) when (e is not OperationCanceledException)
        {
            // Never take the service (and with it telemetry) down over this.
            SidecarLog.Log($"[rigstats-control] Start-up profile apply failed: {e}");
        }
    }

    public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;
}
