namespace SensorSidecar.Control;

/// The design doc's boot-crash guard: a `pending-apply` marker is written
/// before a risky hardware change (CPU limits #189, Curve Optimizer #191)
/// and cleared once the machine has stayed up for `StableAfter`. A marker
/// still present when the service starts means the machine went down while
/// a change was fresh, so `ActiveProfileApplier` leaves the risky parts at
/// their BIOS values for this boot instead of re-applying them.
public sealed class BootCrashGuard : IDisposable
{
    public static readonly TimeSpan DefaultStableAfter = TimeSpan.FromMinutes(3);

    private readonly string _markerPath;
    private readonly TimeSpan _stableAfter;
    private readonly object _lock = new();
    private Timer? _clearTimer;
    private bool _noticeShown;

    /// Reads (and consumes) a marker left by the previous run.
    public BootCrashGuard(string markerPath, TimeSpan stableAfter)
    {
        _markerPath = markerPath;
        _stableAfter = stableAfter;
        TrippedAtStart = File.Exists(markerPath);
        if (TrippedAtStart)
        {
            SidecarLog.Log("[rigstats-control] Boot-crash guard: the last risky change did not survive "
                + $"{stableAfter.TotalMinutes:F0} min — not re-applying CPU limits this boot.");
            TryDelete();
        }
    }

    public bool TrippedAtStart { get; }

    /// Shown in the Control Center until a profile is applied again.
    public string? Notice => TrippedAtStart && !_noticeShown
        ? "The PC restarted shortly after CPU limits were changed, so they were not re-applied at start-up. Apply a profile to use them again."
        : null;

    public void AcknowledgeNotice() => _noticeShown = true;

    /// Call right before a risky write. (Re)starts the stable-uptime clock.
    public void Arm()
    {
        lock (_lock)
        {
            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(_markerPath)!);
                File.WriteAllText(_markerPath, DateTimeOffset.UtcNow.ToString("O"));
            }
            catch (Exception e)
            {
                SidecarLog.Log($"[rigstats-control] Boot-crash guard: marker not written: {e.Message}");
            }
            _clearTimer?.Dispose();
            _clearTimer = new Timer(_ => Clear(), null, _stableAfter, Timeout.InfiniteTimeSpan);
        }
    }

    public bool IsArmed => File.Exists(_markerPath);

    public void Clear()
    {
        lock (_lock)
        {
            _clearTimer?.Dispose();
            _clearTimer = null;
            TryDelete();
        }
    }

    private void TryDelete()
    {
        try
        {
            File.Delete(_markerPath);
        }
        catch
        {
            // Best-effort: a marker that can't be removed only means one
            // over-cautious boot.
        }
    }

    /// Stops the clock but keeps the marker: a service stop mid-window
    /// (often a shutdown) must not count as "stable".
    public void Dispose()
    {
        lock (_lock)
        {
            _clearTimer?.Dispose();
            _clearTimer = null;
        }
    }
}
