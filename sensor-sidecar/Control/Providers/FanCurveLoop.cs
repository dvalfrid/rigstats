using Microsoft.Extensions.Hosting;

namespace SensorSidecar.Control.Providers;

/// Raised by `FanCurveLoop` each tick; `ControlPipeWorker` forwards them to a
/// subscribed client as `safety_tripped` / `fan_duty` events.
public abstract record FanEvent
{
    /// CPU or GPU crossed its critical threshold — every active header was
    /// forced to 100% regardless of its own curve. Edge-triggered: one event
    /// per transition into the critical state, not every tick it stays there.
    public sealed record SafetyTripped(string Reason) : FanEvent;

    /// This tick's commanded duty per header — the UI's live `fan_duty` readout.
    public sealed record DutyUpdate(IReadOnlyDictionary<string, double> DutyByHeader) : FanEvent;
}

/// The ~1 Hz shell around `FanCurveEvaluator`'s pure logic (doc: "Curves are
/// evaluated in the service at ~1 Hz. The UI only edits them.") — reads the
/// same cached sample the telemetry pipe serves (so fan decisions agree with
/// what the dashboard displays, and keep updating with no client connected),
/// evaluates every header `FanProvider` currently has active, and enforces
/// the critical-temperature override before writing anything.
public sealed class FanCurveLoop(IHardwareHost host, FanProvider fanProvider) : BackgroundService
{
    private static readonly TimeSpan TickInterval = TimeSpan.FromSeconds(1);

    // Per header: the config the last decision was made for (a new profile
    // replaces the instance, which drops the stale hysteresis state) and
    // that decision.
    private readonly Dictionary<string, (FanHeaderConfig Config, FanCurveEvaluator.HeaderDecision Decision)> _state = new();
    private bool _criticalActive;

    /// Raised synchronously on the loop's thread — handlers must not block
    /// (the pipe just queues the event for its own writer).
    public event Action<FanEvent>? EventRaised;

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        using var timer = new PeriodicTimer(TickInterval);
        while (await timer.WaitForNextTickAsync(stoppingToken))
        {
            try
            {
                await TickAsync(stoppingToken);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                break;
            }
            catch (Exception e)
            {
                // Mirrors ControlPipeWorker's top-level catch: a BackgroundService
                // that throws out of ExecuteAsync stops the whole host (the
                // default BackgroundServiceExceptionBehavior is StopHost) — one
                // bad tick must not take the service down; the next tick retries.
                SidecarLog.Log($"[rigstats-control] FanCurveLoop tick error: {e}");
            }
        }
    }

    internal async Task TickAsync(CancellationToken ct)
    {
        var active = fanProvider.GetActiveConfig();
        foreach (var id in _state.Keys.Where(id => !active.ContainsKey(id)).ToList())
            _state.Remove(id);
        if (active.Count == 0)
        {
            _criticalActive = false;
            return;
        }

        var sample = await host.GetSampleAsync(ct);

        var criticalReason = FanCurveEvaluator.CriticalReason(sample);
        var critical = criticalReason is not null;
        if (critical && !_criticalActive)
        {
            EventRaised?.Invoke(new FanEvent.SafetyTripped(criticalReason!));
            SidecarLog.Log($"[rigstats-control] Critical temperature override ({criticalReason}): all active fan headers forced to 100%.");
        }
        _criticalActive = critical;

        var dutyByHeader = new Dictionary<string, double>();
        foreach (var (id, config) in active)
        {
            FanCurveEvaluator.HeaderDecision? previous =
                _state.TryGetValue(id, out var prev) && ReferenceEquals(prev.Config, config) ? prev.Decision : null;
            var decision = FanCurveEvaluator.EvaluateHeader(
                config, FanCurveEvaluator.ResolveSource(sample, config.Source), previous);
            _state[id] = (config, decision);

            var duty = critical ? 100.0 : decision.Duty;
            dutyByHeader[id] = duty;
            fanProvider.ApplyDuty(id, duty);
        }

        EventRaised?.Invoke(new FanEvent.DutyUpdate(dutyByHeader));
    }
}
