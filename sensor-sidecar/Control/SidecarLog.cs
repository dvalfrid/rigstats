namespace SensorSidecar.Control;

/// Shared `rigstats-sensor.log` writer — extracted from `SensorWorker`'s
/// original private `Log`/`TruncateLogIfNeeded` so `ControlPipeWorker` (and
/// anything else under `Control/`) can log to the same file without
/// duplicating the path/rotation logic.
public static class SidecarLog
{
    private static readonly string LogPath =
        Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
            "se.codeby.rigstats",
            "rigstats-sensor.log");

    // Telemetry clients log from concurrent tasks (e.g. all disconnecting at
    // service stop); unserialized appends hit sharing violations and the
    // swallowed exception silently dropped lines (#204).
    private static readonly object FileLock = new();

    public static void Log(string message)
    {
        var line = $"[{DateTimeOffset.UtcNow:yyyy-MM-dd HH:mm:ss}Z] {message}";
        Console.Error.WriteLine(line);
        try
        {
            lock (FileLock)
            {
                Directory.CreateDirectory(Path.GetDirectoryName(LogPath)!);
                File.AppendAllText(LogPath, line + Environment.NewLine);
            }
        }
        catch { }
    }

    // Keep the log from growing indefinitely: when it exceeds 512 KB,
    // truncate to the last 500 lines so recent context is always preserved.
    public static void TruncateIfNeeded()
    {
        try
        {
            lock (FileLock)
            {
                if (File.Exists(LogPath) && new FileInfo(LogPath).Length > 512 * 1024)
                {
                    var lines = File.ReadAllLines(LogPath);
                    File.WriteAllLines(LogPath, lines.TakeLast(500));
                }
            }
        }
        catch { }
    }
}
