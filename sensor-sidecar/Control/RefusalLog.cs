namespace SensorSidecar.Control;

/// Throttles the control pipe's "Client rejected" line (#231). A refused app
/// retries, so logging every refusal fills `rigstats-sensor.log` with the
/// same line; a refusal is logged when its reason changes or `Interval` has
/// passed since it was last logged, with the refusals skipped in between.
internal sealed class RefusalLog
{
    internal static readonly TimeSpan Interval = TimeSpan.FromMinutes(10);

    private string? _reason;
    private DateTimeOffset _loggedAt;
    private int _skipped;

    /// The line to log for this refusal, or null when it is a repeat.
    public string? Refused(string reason, DateTimeOffset now)
    {
        if (reason == _reason && now - _loggedAt < Interval)
        {
            _skipped++;
            return null;
        }

        var line = reason == _reason && _skipped > 0
            ? $"Client rejected: {reason} (refused {_skipped} more times since)"
            : $"Client rejected: {reason}";
        _reason = reason;
        _loggedAt = now;
        _skipped = 0;
        return line;
    }

    /// A client was accepted: the next refusal is news again.
    public void Accepted()
    {
        _reason = null;
        _skipped = 0;
    }
}
