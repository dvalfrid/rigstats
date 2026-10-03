using Microsoft.Extensions.Hosting;

namespace SensorSidecar.Control;

/// Doc ("Profile model"): "The service re-applies the active profile at
/// start-up, so profiles work before anyone logs in." Without this, fan
/// curves would be gone after every reboot or service restart until someone
/// re-applied the profile from the UI. Runs once, through the same broker
/// transaction as a UI apply. When the boot-crash guard tripped, the risky
/// parts (CPU limits, Curve Optimizer) are put back to their BIOS values for
/// this boot.
public sealed class ActiveProfileApplier(ProfileStore profiles, ControlBroker broker, BootCrashGuard crashGuard) : IHostedService
{
    public async Task StartAsync(CancellationToken cancellationToken)
    {
        try
        {
            var id = await profiles.GetActiveIdAsync(cancellationToken);
            if (id is null || await profiles.GetAsync(id, cancellationToken) is not { } profile)
                return;
            if (crashGuard.TrippedAtStart)
                profile = WithoutRiskyParts(profile);
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

    /// The risky parts are applied as BIOS values rather than left out:
    /// after a reboot that is a no-op (the SMU has reset), but when only the
    /// service went down, the SMU still holds the risky values — this puts
    /// them back. Both writes are at BIOS values, so neither re-arms the guard.
    internal static Profile WithoutRiskyParts(Profile profile) => new()
    {
        Id = profile.Id,
        Name = profile.Name,
        Icon = profile.Icon,
        Builtin = profile.Builtin,
        Part = new ProfilePart
        {
            PowerPlan = profile.Part.PowerPlan,
            Fan = profile.Part.Fan,
            CpuLimit = new CpuLimitPart(),
            CurveOpt = new CurveOptPart(),
            Gpu = profile.Part.Gpu,
            Aura = profile.Part.Aura,
        },
    };

    public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;
}
