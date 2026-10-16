using Microsoft.Extensions.Hosting;

namespace SensorSidecar.Control;

/// The cross-provider part of `docs/control-architecture.md`'s safety table.
/// The fan critical-temp and sensor-loss overrides live in `FanCurveLoop`
/// (#188); the boot-crash guard is `BootCrashGuard` (#189).
/// What this provides:
///
/// - Release-on-stop: every provider hands control back to firmware when the
///   service stops, cleanly or otherwise.
/// - Dry-run: `--dry-run` makes `ControlBroker`'s applies log-only.
public sealed class SafetyGuard(IEnumerable<IControlProvider> providers, bool dryRun) : IHostedService
{
    /// Why control is handed back, for the log line (#385).
    public const string ServiceStops = "the service stops";
    public const string AskedFromControlCenter = "the Control Center asked for it";

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

    /// Every provider hands control back. What was handed back goes into the
    /// service log in one line, with why (#385) — nothing when there was
    /// nothing to hand back, so the backstop call after a clean stop stays
    /// quiet. A provider that fails is logged and the rest still get their turn.
    public void ReleaseAllToFirmware(string reason = ServiceStops)
    {
        var handedBack = new List<string>();
        foreach (var provider in providers)
        {
            try
            {
                if (provider.ReleaseToFirmware() is { } what)
                    handedBack.Add($"{provider.Domain}: {what}");
            }
            catch (Exception e)
            {
                // Best-effort: one provider failing to release must not stop
                // the rest from getting a chance to — but it must show: this
                // is when the hardware may be left on RIGStats' settings.
                SidecarLog.Log($"[rigstats-control] {provider.Domain} could not be handed back to the firmware ({reason}): {e.Message}");
            }
        }
        if (Line(reason, DryRun, handedBack) is { } line)
            SidecarLog.Log(line);
    }

    /// The log line for one release, or null when nothing was handed back.
    internal static string? Line(string reason, bool dryRun, IReadOnlyList<string> handedBack) =>
        handedBack.Count == 0
            ? null
            : $"[rigstats-control] Handed back to the firmware because {reason}"
                + $"{(dryRun ? " (dry-run: nothing written)" : "")}: {string.Join("; ", handedBack)}.";
}
