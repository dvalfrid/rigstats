using System.Globalization;

namespace SensorSidecar.Control;

/// One Windows Application-log entry about a crashed process: ".NET Runtime"
/// 1026 (with the managed stack) or "Application Error" 1000 (the faulting
/// module and exception code).
public sealed record CrashEvent(DateTimeOffset Time, int Id, string Message);

/// A crash dump Windows Error Reporting wrote for the service (#220).
public sealed record CrashDump(string Name, DateTimeOffset Time, long Bytes);

/// Earlier crashes of the service, from the Windows event log into
/// `rigstats-sensor.log` at the next start (#242). A native fault — an access
/// violation inside a driver call — ends the process before any .NET handler
/// runs, so the service's own log only showed a new start. Windows records
/// the crash with the stack; this copies it over once, so a diagnostics
/// export shows it.
public static class CrashReport
{
    public const string ServiceExe = "rigstats-sensor.exe";
    public const int DotNetRuntimeId = 1026;
    public const int ApplicationErrorId = 1000;

    /// How far back the first run (no mark yet) looks.
    public static readonly TimeSpan FirstLookBack = TimeSpan.FromDays(7);

    /// Stack frames kept per crash: enough to see where it happened.
    private const int MaxFrames = 12;

    /// Windows writes both entries for one crash within moments of each
    /// other; the runtime's (with the stack) is enough when it is there.
    private static readonly TimeSpan SameCrash = TimeSpan.FromSeconds(10);

    /// Log lines for this service's crashes after `after`, oldest first.
    public static List<string> Lines(IEnumerable<CrashEvent> events, DateTimeOffset after)
    {
        var ours = events.Where(e => e.Time > after && IsService(e)).OrderBy(e => e.Time).ToList();
        var withStack = ours.Where(e => e.Id == DotNetRuntimeId).Select(e => e.Time).ToList();
        var lines = new List<string>();
        foreach (var e in ours)
        {
            if (e.Id == ApplicationErrorId && withStack.Any(t => (t - e.Time).Duration() <= SameCrash))
                continue;
            var when = e.Time.ToLocalTime().ToString("yyyy-MM-dd HH:mm:ss zzz", CultureInfo.InvariantCulture);
            if (e.Id == DotNetRuntimeId)
                lines.Add($"[rigstats-sensor] An earlier run crashed at {when}: {Field(e.Message, "Description:") ?? "unknown cause"}"
                    + (Field(e.Message, "Exception Info:") is { } info ? $" {info}" : "")
                    + Stack(e.Message));
            else if (e.Id == ApplicationErrorId)
                lines.Add($"[rigstats-sensor] An earlier run crashed at {when}: exception {Field(e.Message, "Exception code:") ?? "?"}"
                    + $" in {Field(e.Message, "Faulting module name:")?.Split(',')[0] ?? "?"}"
                    + $" at offset {Field(e.Message, "Fault offset:") ?? "?"}");
        }
        return lines;
    }

    /// Log lines for the crash dumps written after `after`, oldest first.
    /// The dump itself stays in the administrators-only folder (#220): the
    /// log only says that it exists, so a diagnostics export shows it and
    /// it can be asked for.
    public static List<string> DumpLines(IEnumerable<CrashDump> dumps, DateTimeOffset after, string folder) =>
        dumps.Where(d => d.Time > after)
            .OrderBy(d => d.Time)
            .Select(d => $"[rigstats-sensor] Crash dump written at "
                + $"{d.Time.ToLocalTime().ToString("yyyy-MM-dd HH:mm:ss zzz", CultureInfo.InvariantCulture)}: "
                + $"{Path.Combine(folder, d.Name)} ({(d.Bytes / 1048576.0).ToString("0.0", CultureInfo.InvariantCulture)} MB, administrators only)")
            .ToList();

    /// The crash is this service's — the release build or a dev build.
    private static bool IsService(CrashEvent e)
    {
        var name = e.Id == DotNetRuntimeId ? Field(e.Message, "Application:") : Field(e.Message, "Faulting application name:");
        return name is not null && name.StartsWith(ServiceExe, StringComparison.OrdinalIgnoreCase);
    }

    private static string? Field(string message, string label)
    {
        foreach (var raw in message.Split('\n'))
        {
            var line = raw.Trim();
            if (line.StartsWith(label, StringComparison.OrdinalIgnoreCase))
                return line[label.Length..].Trim();
        }
        return null;
    }

    private static string Stack(string message)
    {
        var frames = message.Split('\n')
            .Select(l => l.Trim())
            .SkipWhile(l => !l.Equals("Stack:", StringComparison.OrdinalIgnoreCase))
            .Skip(1)
            .TakeWhile(l => l.StartsWith("at ", StringComparison.Ordinal))
            .ToList();
        if (frames.Count == 0)
            return "";
        var shown = frames.Take(MaxFrames).Select(f => $"\n    {f}");
        return string.Concat(shown) + (frames.Count > MaxFrames ? $"\n    … {frames.Count - MaxFrames} more frames" : "");
    }

    /// The time of the last crash already logged; null when none was yet.
    public static DateTimeOffset? ReadMark(string path)
    {
        try
        {
            return File.Exists(path)
                && DateTimeOffset.TryParse(File.ReadAllText(path).Trim(), CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out var mark)
                ? mark
                : null;
        }
        catch
        {
            return null;
        }
    }

    public static void WriteMark(string path, DateTimeOffset time)
    {
        try
        {
            File.WriteAllText(path, time.ToString("O", CultureInfo.InvariantCulture));
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-sensor] Crash report mark not saved: {e.Message}");
        }
    }
}
