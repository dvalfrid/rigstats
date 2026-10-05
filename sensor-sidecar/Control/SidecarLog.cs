namespace SensorSidecar.Control;

/// Shared `rigstats-sensor.log` writer — extracted from `SensorWorker`'s
/// original private `Log`/`TruncateLogIfNeeded` so `ControlPipeWorker` (and
/// anything else under `Control/`) can log to the same file without
/// duplicating the path/rotation logic.
public static class SidecarLog
{
    private static readonly LogFile Sink = new(
        Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
            "se.codeby.rigstats",
            "rigstats-sensor.log"));

    /// Off when the data folder could not be secured (`DataDirectory`): a
    /// SYSTEM process must not append to a file someone else may redirect.
    public static bool FileEnabled { get; set; } = true;

    public static void Log(string message)
    {
        // Local time with its UTC offset, like the app's log (#219) — the two
        // line up in a diagnostics export without converting.
        var line = $"[{DateTimeOffset.Now:yyyy-MM-dd HH:mm:ss zzz}] {message}";
        Console.Error.WriteLine(line);
        if (!FileEnabled)
            return;
        try
        {
            Sink.Append(line);
        }
        catch { }
    }

    /// At service start; `Log` also keeps the file bounded while it runs.
    public static void TruncateIfNeeded()
    {
        try
        {
            Sink.TruncateIfNeeded();
        }
        catch { }
    }
}

/// An append-only log file kept bounded while it is written (#223): every
/// `checkEveryBytes` appended, a file over `maxBytes` is cut to its last
/// `keepLines` lines, so recent context is always kept. Instance-based so
/// tests get their own file and limits.
internal sealed class LogFile(
    string path,
    long maxBytes = 512 * 1024,
    int keepLines = 500,
    long checkEveryBytes = 64 * 1024)
{
    // Telemetry clients log from concurrent tasks (e.g. all disconnecting at
    // service stop); unserialized appends hit sharing violations and the
    // swallowed exception silently dropped lines (#204).
    private readonly object _lock = new();
    private long _sinceCheck;

    public void Append(string line)
    {
        lock (_lock)
        {
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.AppendAllText(path, line + Environment.NewLine);
            _sinceCheck += line.Length + Environment.NewLine.Length;
            if (_sinceCheck < checkEveryBytes)
                return;
            _sinceCheck = 0;
            TruncateLocked();
        }
    }

    public void TruncateIfNeeded()
    {
        lock (_lock)
            TruncateLocked();
    }

    private void TruncateLocked()
    {
        if (!File.Exists(path) || new FileInfo(path).Length <= maxBytes)
            return;
        // Through a temp file, so a crash mid-truncate can't leave the log empty.
        var temp = path + ".tmp";
        File.WriteAllLines(temp, File.ReadAllLines(path).TakeLast(keepLines));
        File.Move(temp, path, overwrite: true);
    }
}
