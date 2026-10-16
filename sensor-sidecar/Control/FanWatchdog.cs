using Microsoft.Extensions.Hosting;
using SensorSidecar.Control.Providers;

namespace SensorSidecar.Control;

/// Acts when the fan curve loop stops ticking (#222). Once a curve is active
/// the loop is the only thing that moves fan duty — and the critical-
/// temperature override lives in it — so a hung hardware call would leave
/// every header at its last duty while temperatures rise. The watchdog runs
/// on its own thread (not the thread pool, which may be what's starved).
///
/// First it ends the hardware worker (#374): a hung LibreHardwareMonitor or
/// driver call lives there, and a new worker lets the loop write again within
/// seconds, with the curves still active. Only when that doesn't help —
/// `MaxRestarts` within `RestartWindow`, so the hang is in the service
/// itself — does it hand the fans back to firmware and fail fast: the service
/// manager restarts the service and `ActiveProfileApplier` re-applies the
/// profile. A hung call can't be cancelled in-process.
public sealed class FanWatchdog : IHostedService, IDisposable
{
    internal static readonly TimeSpan StallLimit = TimeSpan.FromSeconds(10);
    private static readonly TimeSpan CheckInterval = TimeSpan.FromSeconds(2);
    private static readonly TimeSpan DefaultReleaseLimit = TimeSpan.FromSeconds(3);

    private readonly Func<long> _nowMs;
    private readonly Func<long> _lastTickMs;
    private readonly Func<bool> _curvesActive;
    private readonly Action _release;
    private readonly Action<string> _restartHardware;
    private readonly Action<string> _failFast;
    private readonly TimeSpan _releaseLimit;
    private readonly ManualResetEventSlim _stop = new();
    private Thread? _thread;
    private long _lastCheckMs;
    private long _resumedAtMs;
    private readonly Queue<long> _restarts = new();

    /// Worker restarts within this window before the service is failed.
    internal static readonly TimeSpan RestartWindow = TimeSpan.FromMinutes(2);
    internal const int MaxRestarts = 2;

    public FanWatchdog(FanCurveLoop loop, FanProvider fans, IHardwareHost hardware)
        : this(
            () => Environment.TickCount64,
            () => loop.LastTickMs,
            () => fans.GetActiveConfig().Count > 0,
            () =>
            {
                // The watchdog's own release, outside SafetyGuard: logged here (#385).
                if (fans.ReleaseToFirmware() is { } what)
                    SidecarLog.Log($"[rigstats-control] Handed back to the firmware because the fan loop stalled: fan: {what}.");
            },
            hardware.RestartHardware,
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
        Action<string> restartHardware,
        Action<string> failFast,
        TimeSpan releaseLimit)
    {
        _nowMs = nowMs;
        _lastTickMs = lastTickMs;
        _curvesActive = curvesActive;
        _release = release;
        _restartHardware = restartHardware;
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
        while (_restarts.Count > 0 && now - _restarts.Peek() > RestartWindow.TotalMilliseconds)
            _restarts.Dequeue();
        if (_restarts.Count < MaxRestarts)
        {
            // The hang is most likely in the hardware worker: end it. The
            // loop's hung call fails, the next one starts a new worker, and
            // the curves carry on — no profile to re-apply.
            _restarts.Enqueue(now);
            SidecarLog.Log($"[rigstats-control] {stalled} — restarting the hardware worker (#374).");
            try
            {
                _restartHardware(stalled);
            }
            catch (Exception e)
            {
                SidecarLog.Log($"[rigstats-control] Restarting the hardware worker failed: {e.Message}");
            }
            _resumedAtMs = now; // a fresh window for the new worker
            return false;
        }
        SidecarLog.Log($"[rigstats-control] {stalled} again after {MaxRestarts} hardware worker restarts — releasing fans to firmware and restarting the service.");
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
        })
        { IsBackground = true, Name = "FanWatchdog release" };
        releasing.Start();
        if (!releasing.Join(_releaseLimit))
            SidecarLog.Log($"[rigstats-control] Releasing fans did not finish within {_releaseLimit.TotalSeconds:0} s.");
        _failFast($"RIGStats: {stalled}.");
        return true;
    }

    public void Dispose() => _stop.Dispose();
}
