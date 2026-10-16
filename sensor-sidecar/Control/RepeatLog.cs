namespace SensorSidecar.Control;

/// Throttles a log line that repeats (#231, #378). Something that fails over
/// and over — a refused app that retries, a sensor read that keeps failing
/// while the app reconnects every second — would otherwise fill the bounded
/// `rigstats-sensor.log` with one line until the cause has rotated out. A line
/// is logged when its reason changes or `Interval` has passed since it was
/// last logged, with the repeats skipped in between. Thread-safe: telemetry
/// clients fail from their own tasks.
internal sealed class RepeatLog(string prefix, string verb)
{
    internal static readonly TimeSpan Interval = TimeSpan.FromMinutes(10);

    private readonly object _lock = new();
    private string? _reason;
    private DateTimeOffset _loggedAt;
    private int _skipped;

    /// Whether a failure is being held — logged and not yet `Reset`.
    public bool Active
    {
        get
        {
            lock (_lock)
                return _reason is not null;
        }
    }

    /// The line to log for this occurrence, or null when it is a repeat.
    public string? Line(string reason, DateTimeOffset now)
    {
        lock (_lock)
        {
            if (reason == _reason && now - _loggedAt < Interval)
            {
                _skipped++;
                return null;
            }

            var line = reason == _reason && _skipped > 0
                ? $"{prefix}: {reason} ({verb} {_skipped} more times since)"
                : $"{prefix}: {reason}";
            _reason = reason;
            _loggedAt = now;
            _skipped = 0;
            return line;
        }
    }

    /// It worked: the next failure is news again. True when a failure was
    /// being held, so the caller can say it recovered.
    public bool Reset()
    {
        lock (_lock)
        {
            var wasActive = _reason is not null;
            _reason = null;
            _skipped = 0;
            return wasActive;
        }
    }
}
