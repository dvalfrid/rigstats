using Microsoft.Extensions.Hosting;
using SensorSidecar.Control.Providers;

namespace SensorSidecar.Control;

/// Restarts the service when the fan curve loop stops ticking (#222). Once a
/// curve is active the loop is the only thing that moves fan duty — and the
/// critical-temperature override lives in it — so a hung LHM/PawnIO call or a
/// deadlock would leave every header at its last duty while temperatures
/// rise, with the process still alive for the service manager. The watchdog
/// runs on its own thread (not the thread pool, which may be what's starved),
/// hands the fans back to firmware, and fails fast: the service manager
/// restarts the service and `ActiveProfileApplier` re-applies the profile —
/// the recovery path every start already exercises. A hung native call can't
/// be cancelled in-process.
public sealed class FanWatchdog : IHostedService, IDisposable
{
    internal static readonly TimeSpan StallLimit = TimeSpan.FromSeconds(10);
    private static readonly TimeSpan CheckInterval = TimeSpan.FromSeconds(2);
    private static readonly TimeSpan DefaultReleaseLimit = TimeSpan.FromSeconds(3);

    private readonly Func<long> _nowMs;
    private readonly Func<long> _lastTickMs;
    private readonly Func<bool> _curvesActive;
    private readonly Action _release;
    private readonly Action<string> _failFast;
    private readonly TimeSpan _releaseLimit;
    private readonly ManualResetEventSlim _stop = new();
    private Thread? _thread;
    private long _lastCheckMs;
    private long _resumedAtMs;

    public FanWatchdog(FanCurveLoop loop, FanProvider fans)
        : this(
            () => Environment.TickCount64,
            () => loop.LastTickMs,
            () => fans.GetActiveConfig().Count > 0,
            fans.ReleaseToFirmware,
            Environment.FailFast,
            DefaultReleaseLimit)
    {
    }

    // Seam for tests: a fake clock, and release/fail-fast that only record.
    internal FanWatchdog(
        Func<long> nowMs,
        Func<long> lastTickMs,
        Func<bool> curvesActive,
        Action release,
        Action<string> failFast,
        TimeSpan releaseLimit)
    {
        _nowMs = nowMs;
        _lastTickMs = lastTickMs;
        _curvesActive = curvesActive;
        _release = release;
        _failFast = failFast;
        _releaseLimit = releaseLimit;
        _lastCheckMs = nowMs();
    }

    public Task StartAsync(CancellationToken cancellationToken)
    {
        _thread = new Thread(Run) { IsBackground = true, Name = "FanWatchdog" };
        _thread.Start();
        return Task.CompletedTask;
    }

    public Task StopAsync(CancellationToken cancellationToken)
    {
        _stop.Set();
        _thread?.Join(CheckInterval);
        return Task.CompletedTask;
    }

    private void Run()
    {
        while (!_stop.Wait(CheckInterval))
        {
            if (Check())
                return;
        }
    }

    /// One check. True when the loop had stalled and the service was failed.
    internal bool Check()
    {
        // The clock counts sleep: after the PC wakes, the last tick looks
        // hours old until the loop's next one. A gap in these checks
        // themselves means that (or that this thread was held up too), so the
        // loop gets a fresh window rather than a restart on every wake.
        var now = _nowMs();
        if (now - _lastCheckMs > StallLimit.TotalMilliseconds)
            _resumedAtMs = now;
        _lastCheckMs = now;

        // No active curve: the firmware is in control, nothing to guard.
        if (!_curvesActive())
            return false;
        var stalledMs = now - Math.Max(_lastTickMs(), _resumedAtMs);
        if (stalledMs < StallLimit.TotalMilliseconds)
            return false;

        var stalled = $"Fan loop stalled for {stalledMs / 1000} s";
        SidecarLog.Log($"[rigstats-control] {stalled} — releasing fans to firmware and restarting the service.");
        // On a helper thread with a limit: if the hardware lock is what hangs,
        // the release hangs too, and the restart must still happen.
        var releasing = new Thread(() =>
        {
            try
            {
                _release();
            }
            catch (Exception e)
            {
                SidecarLog.Log($"[rigstats-control] Releasing fans after the stall failed: {e.Message}");
            }
        }) { IsBackground = true, Name = "FanWatchdog release" };
        releasing.Start();
        if (!releasing.Join(_releaseLimit))
            SidecarLog.Log($"[rigstats-control] Releasing fans did not finish within {_releaseLimit.TotalSeconds:0} s.");
        _failFast($"RIGStats: {stalled}.");
        return true;
    }

    public void Dispose() => _stop.Dispose();
}
