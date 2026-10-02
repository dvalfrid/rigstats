using Microsoft.Extensions.Hosting;

namespace SensorSidecar.Control;

/// The cross-provider part of `docs/control-architecture.md`'s safety table.
/// The fan critical-temp and sensor-loss overrides live in `FanCurveLoop`
/// (#188); the boot-crash guard arrives with `CpuLimitProvider` (#189).
/// What this provides:
///
/// - Release-on-stop: every provider hands control back to firmware when the
///   service stops, cleanly or otherwise.
/// - Dry-run: `--dry-run` makes `ControlBroker`'s applies log-only.
public sealed class SafetyGuard(IEnumerable<IControlProvider> providers, bool dryRun) : IHostedService
{
    public bool DryRun { get; } = dryRun;

    public Task StartAsync(CancellationToken cancellationToken) => Task.CompletedTask;

    /// Called on ordinary service stop; `Program.cs` also calls
    /// <see cref="ReleaseAllToFirmware"/> from a top-level `finally` so an
    /// unhandled exception in the host still releases control.
    public Task StopAsync(CancellationToken cancellationToken)
    {
        ReleaseAllToFirmware();
        return Task.CompletedTask;
    }

    public void ReleaseAllToFirmware()
    {
        foreach (var provider in providers)
        {
            try
            {
                provider.ReleaseToFirmware();
            }
            catch
            {
                // Best-effort: one provider failing to release must not stop
                // the rest from getting a chance to.
            }
        }
    }
}
