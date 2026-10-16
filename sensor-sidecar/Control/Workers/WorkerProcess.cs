using System.Diagnostics;
using System.Text;
using System.Text.RegularExpressions;

namespace SensorSidecar.Control.Workers;

/// A worker as a real child process: the service's own exe with a worker
/// argument (`--gpu-worker`, #370), stdio redirected (UTF-8 without a
/// byte-order mark, so the first line is plain JSON), no window. It inherits
/// the service's account. Its log lines arrive on stderr and go into the
/// service's log as `[<name>] …` — two processes appending to one file would
/// collide.
public sealed class WorkerProcess : IWorker
{
    private const int MaxForwardedLines = 200;
    private static readonly Regex Timestamp = new(@"^\[\d{4}-\d{2}-\d{2} [^\]]+\] ", RegexOptions.Compiled);

    private readonly Process _process;
    private int _forwarded;

    private WorkerProcess(Process process) => _process = process;

    /// `name` tags its log lines ("gpu-worker"); `log` replaces the service
    /// log (tests).
    public static WorkerProcess Start(string exe, string arguments, string name, Action<string>? log = null)
    {
        var utf8 = new UTF8Encoding(false);
        var info = new ProcessStartInfo(exe, arguments)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardInputEncoding = utf8,
            StandardOutputEncoding = utf8,
            StandardErrorEncoding = utf8,
        };
        var process = new Process { StartInfo = info };
        var worker = new WorkerProcess(process);
        var forward = log ?? (line => SidecarLog.Log($"[{name}] {line}"));
        process.ErrorDataReceived += (_, e) =>
        {
            if (string.IsNullOrWhiteSpace(e.Data) || Interlocked.Increment(ref worker._forwarded) > MaxForwardedLines)
                return;
            forward(Timestamp.Replace(e.Data, "").TrimEnd());
        };
        if (!process.Start())
            throw new InvalidOperationException($"{exe} did not start.");
        // Tied to the service: if the service ends — however it ends — so
        // does the worker, even one stuck inside a driver call.
        if (KillOnCloseJob.Shared?.Add(process) != true)
            SidecarLog.Log($"[rigstats-sensor] The {name} could not be tied to the service (job object); it still ends when the service closes its input.");
        process.BeginErrorReadLine();
        return worker;
    }

    /// The process id, for logs and tests.
    public int Id => _process.Id;
    public TextWriter Input => _process.StandardInput;
    public TextReader Output => _process.StandardOutput;
    public bool HasExited => _process.HasExited;
    public int? ExitCode => _process.HasExited ? _process.ExitCode : null;
    public bool WaitForExit(TimeSpan timeout) => _process.WaitForExit(timeout);
    public void Kill() => _process.Kill(entireProcessTree: true);

    public void Dispose()
    {
        try
        {
            if (!_process.HasExited)
                _process.Kill(entireProcessTree: true);
        }
        catch
        {
            // Already gone.
        }
        _process.Dispose();
    }
}
