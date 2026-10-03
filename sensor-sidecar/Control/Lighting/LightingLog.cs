namespace SensorSidecar.Control.Lighting;

/// Discovery log lines (devices found, devices that don't answer) for the
/// first scan only: the lighting devices are scanned again whenever the
/// Control Center opens or a profile applies, and repeating every line each
/// time would bury the diagnostics log. Later scans log only what changed
/// (LightingProvider).
public static class LightingLog
{
    private static int _quiet;

    public static void Discovery(string message)
    {
        if (Volatile.Read(ref _quiet) == 0)
            SidecarLog.Log(message);
    }

    /// Discovery lines are dropped until the returned scope is disposed.
    public static IDisposable Quiet()
    {
        Interlocked.Increment(ref _quiet);
        return new Scope();
    }

    private sealed class Scope : IDisposable
    {
        private int _disposed;

        public void Dispose()
        {
            if (Interlocked.Exchange(ref _disposed, 1) == 0)
                Interlocked.Decrement(ref _quiet);
        }
    }
}
