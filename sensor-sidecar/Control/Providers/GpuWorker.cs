using System.Collections.Concurrent;
using System.Diagnostics;
using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

namespace SensorSidecar.Control.Providers;

// The GPU driver libraries (AMD's ADLX, NVIDIA's NVML) run in a child process
// of the service, `rigstats-sensor.exe --gpu-worker` (#370). A native fault in
// a vendor DLL ends the process it runs in, and .NET can't catch it — on
// 2026-10-15 a Windows Update that replaced the Adrenalin driver made ADLX
// crash the whole service (sensors, fans, lighting) every time the app
// connected. In the worker, a crash or a hang costs only the GPU power
// limits: the service logs it, shows the reason in the GPU tab, and doesn't
// start the worker again until Windows restarts or a driver changes.

/// The GPU driver can't be used: its worker crashed or hung (#370). The
/// message says why and when it is tried again (the service log and the
/// capability's reason); `GpuProblems` turns it into what the user sees.
public sealed class GpuDriverCrashedException(string message) : InvalidOperationException(message)
{
    /// The driver stopped answering rather than crashing.
    public bool Hung => Message.Contains("did not answer", StringComparison.Ordinal);
}

/// What the user sees when the GPU power limits are off because of the
/// driver (#370): what happened, that the rest works, what to do — and
/// buttons for the driver download, diagnostics and the help page.
public static class GpuProblems
{
    public const string AmdDriverUrl = "https://www.amd.com/en/support/download/drivers.html";
    public const string NvidiaDriverUrl = "https://www.nvidia.com/en-us/drivers/";
    public const string AmdHelpUrl = "https://rigstats.app/#faq-amd-driver";
    public const string HelpUrl = "https://github.com/dvalfrid/rigstats/blob/main/docs/troubleshooting.md#control-center";

    private const string LimitChangedNote =
        " RIGStats had changed the power limit: it stays as it is until the driver works again, or reset it in AMD Software or the NVIDIA app.";

    /// The worker crashed or hung: an error until a restart or a new driver.
    public static Problem DriverDown(GpuDriverCrashedException e, string drivers, bool limitChanged) => new()
    {
        Id = e.Hung ? "gpu_driver_hung" : "gpu_driver_crashed",
        Severity = ProblemSeverity.Error,
        Title = e.Hung ? "The graphics driver stopped answering" : "The graphics driver crashed",
        Message = $"RIGStats asked the graphics driver for its power limit and the driver {(e.Hung ? "stopped answering" : "crashed")}. "
            + "Sensors, fans and lighting keep working — only the GPU power limit is off. "
            + "This usually happens after Windows Update replaced the graphics driver: install the latest driver, then restart Windows. "
            + "RIGStats tries again after a restart or a driver update."
            + (limitChanged ? LimitChangedNote : ""),
        Actions = Actions(drivers),
        Detail = $"{e.Message} Drivers: {drivers}.",
    };

    /// The driver answered with an error (not a crash): worth attention,
    /// and it may pass by itself.
    public static Problem DriverFailed(string error, string drivers, bool limitChanged) => new()
    {
        Id = "gpu_driver_failed",
        Severity = ProblemSeverity.Attention,
        Title = "The graphics driver didn't answer",
        Message = "The graphics driver didn't answer when asked for its power limit, so the GPU power limit is off for now. "
            + "Sensors, fans and lighting keep working. If it lasts, install the latest graphics driver and restart Windows."
            + (limitChanged ? LimitChangedNote : ""),
        Actions = Actions(drivers),
        Detail = $"{error} Drivers: {drivers}.",
    };

    private static IReadOnlyList<ProblemAction> Actions(string drivers)
    {
        var actions = new List<ProblemAction>();
        var amd = Installed(drivers, "amdadlx64.dll");
        if (amd)
            actions.Add(new ProblemAction { Kind = ProblemActionKind.OpenUrl, Label = "Get the AMD driver", Target = AmdDriverUrl });
        if (Installed(drivers, "nvml.dll"))
            actions.Add(new ProblemAction { Kind = ProblemActionKind.OpenUrl, Label = "Get the NVIDIA driver", Target = NvidiaDriverUrl });
        actions.Add(new ProblemAction { Kind = ProblemActionKind.CollectDiagnostics, Label = "Collect diagnostics…" });
        actions.Add(new ProblemAction { Kind = ProblemActionKind.OpenUrl, Label = "Read more", Target = amd ? AmdHelpUrl : HelpUrl });
        return actions;
    }

    /// Whether `drivers` (from `GpuCrashGuard.DriverFiles`) names the file
    /// with a version: "amdadlx64.dll 1.5.0.124" yes, "nvml.dll -" no.
    public static bool Installed(string drivers, string file) =>
        drivers.Split(';', StringSplitOptions.TrimEntries).Any(d => d.StartsWith(file + " ", StringComparison.OrdinalIgnoreCase) && d != file + " -");
}

/// Remembers a worker crash for this boot and these driver files, so a
/// service restart doesn't run straight into the same crash.
public static class GpuCrashGuard
{
    private static readonly TimeSpan SameBootTolerance = TimeSpan.FromMinutes(2);

    public static string Marker(DateTimeOffset bootTime, string drivers, string reason) =>
        $"{bootTime.ToString("O", CultureInfo.InvariantCulture)}|{drivers}|{reason}";

    /// The remembered reason when `marker` is from this boot and these
    /// driver files; null otherwise — another boot or a changed driver gets
    /// a new try.
    public static string? CrashedBefore(string? marker, DateTimeOffset bootTime, string drivers)
    {
        if (marker?.Trim().Split('|', 3) is not [var boot, var markedDrivers, var reason])
            return null;
        if (!DateTimeOffset.TryParse(boot, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out var markedBoot))
            return null;
        return (markedBoot - bootTime).Duration() <= SameBootTolerance && markedDrivers == drivers ? reason : null;
    }

    /// The installed GPU driver libraries and their versions, read from the
    /// files (nothing is loaded): "amdadlx64.dll 1.5.0.124; nvml.dll -".
    public static string DriverFiles() => string.Join("; ", DriverPaths().Select(p =>
        $"{Path.GetFileName(p.First())} {p.Select(Version).FirstOrDefault(v => v is not null) ?? "-"}"));

    /// Whether any GPU driver library is installed; without one there is
    /// nothing for a worker to do.
    public static bool AnyDriverInstalled() => DriverPaths().Any(p => p.Any(File.Exists));

    private static IEnumerable<string[]> DriverPaths()
    {
        var system = Environment.SystemDirectory;
        yield return [Path.Combine(system, "amdadlx64.dll")];
        yield return
        [
            Path.Combine(system, "nvml.dll"),
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), @"NVIDIA Corporation\NVSMI\nvml.dll"),
        ];
    }

    private static string? Version(string path)
    {
        try
        {
            return File.Exists(path) ? FileVersionInfo.GetVersionInfo(path).FileVersion?.Trim() : null;
        }
        catch
        {
            return null;
        }
    }
}

/// One request to the worker; `Op` is one of the `GpuWorkerProtocol.Op*`.
public sealed record GpuWorkerRequest(int Id, string Op, string? Adapter = null, int Value = 0);

/// The worker's answer: `Result` when `Ok`, else `Error` (the driver's own
/// failure, like "ADLX_FAIL" — not a crash).
public sealed record GpuWorkerResponse(int Id, bool Ok, JsonNode? Result = null, string? Error = null);

/// One JSON line per request and per answer, over the worker's stdin/stdout.
public static class GpuWorkerProtocol
{
    public const string OpAdapters = "adapters";
    public const string OpGet = "get";
    public const string OpSet = "set";
    public const string OpIsAtFactory = "is_at_factory";
    public const string OpReset = "reset";
    public const string OpEnd = "end";

    private static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
    };

    public static string Encode(GpuWorkerRequest request) => JsonSerializer.Serialize(request, Options);

    public static string Encode(GpuWorkerResponse response) => JsonSerializer.Serialize(response, Options);

    public static GpuWorkerRequest DecodeRequest(string line) =>
        JsonSerializer.Deserialize<GpuWorkerRequest>(line, Options) ?? throw new JsonException("empty request");

    public static GpuWorkerResponse DecodeResponse(string line) =>
        JsonSerializer.Deserialize<GpuWorkerResponse>(line, Options) ?? throw new JsonException("empty response");

    public static JsonNode ToJson(GpuPowerAdapter a) => new JsonObject
    {
        ["id"] = a.Id,
        ["name"] = a.Name,
        ["min"] = a.Min,
        ["max"] = a.Max,
        ["step"] = a.Step,
        ["default"] = a.Default,
    };

    public static GpuPowerAdapter FromJson(JsonNode node) => new(
        (string)node["id"]!, (string)node["name"]!, (int)node["min"]!, (int)node["max"]!, (int)node["step"]!, (int)node["default"]!);
}

/// The worker's side: answers requests with the real driver API until the
/// service says "end" or closes stdin (it went away), then ends the driver
/// session. A driver failure becomes an error answer; only a native fault
/// ends the process — which is the point of running it here.
public static class GpuWorkerLoop
{
    public static void Run(IGpuPowerApi? api, TextReader input, TextWriter output, Action? beforeFirstRequest = null)
    {
        var first = true;
        while (input.ReadLine() is { } line)
        {
            GpuWorkerRequest request;
            try
            {
                request = GpuWorkerProtocol.DecodeRequest(line);
            }
            catch (Exception e)
            {
                output.WriteLine(GpuWorkerProtocol.Encode(new GpuWorkerResponse(0, false, Error: $"Unreadable request: {e.Message}")));
                output.Flush();
                continue;
            }
            if (first)
            {
                first = false;
                beforeFirstRequest?.Invoke();
            }
            output.WriteLine(GpuWorkerProtocol.Encode(Handle(api, request)));
            output.Flush();
            if (request.Op == GpuWorkerProtocol.OpEnd)
                break;
        }
        try
        {
            api?.EndSession();
        }
        catch
        {
            // Ending anyway.
        }
    }

    public static GpuWorkerResponse Handle(IGpuPowerApi? api, GpuWorkerRequest request)
    {
        try
        {
            JsonNode? result = request.Op switch
            {
                GpuWorkerProtocol.OpAdapters => new JsonArray((api?.Adapters() ?? []).Select(GpuWorkerProtocol.ToJson).ToArray()),
                GpuWorkerProtocol.OpGet => Api(api).GetPowerLimit(Adapter(request)),
                GpuWorkerProtocol.OpSet => Done(() => Api(api).SetPowerLimit(Adapter(request), request.Value)),
                GpuWorkerProtocol.OpIsAtFactory => Api(api).IsAtFactory(Adapter(request)),
                GpuWorkerProtocol.OpReset => Done(() => Api(api).ResetToFactory(Adapter(request))),
                GpuWorkerProtocol.OpEnd => Done(() => api?.EndSession()),
                _ => throw new InvalidOperationException($"Unknown request '{request.Op}'."),
            };
            return new GpuWorkerResponse(request.Id, true, result);
        }
        catch (Exception e)
        {
            return new GpuWorkerResponse(request.Id, false, Error: e.Message);
        }
    }

    private static IGpuPowerApi Api(IGpuPowerApi? api) =>
        api ?? throw new InvalidOperationException("No GPU driver with power limit control is installed.");

    private static string Adapter(GpuWorkerRequest request) =>
        request.Adapter ?? throw new InvalidOperationException($"'{request.Op}' needs an adapter.");

    private static JsonNode? Done(Action action)
    {
        action();
        return null;
    }
}

/// The running worker, as the client sees it — a seam so the client is
/// testable without a real process.
public interface IGpuWorker : IDisposable
{
    TextWriter Input { get; }
    TextReader Output { get; }
    bool HasExited { get; }

    /// The exit code once it has exited, else null.
    int? ExitCode { get; }

    bool WaitForExit(TimeSpan timeout);
    void Kill();
}

/// The service's side: `IGpuPowerApi` over the worker. The worker starts on
/// the first call and stays for the next. Every call has a time limit; a
/// worker that crashes, hangs or answers nonsense is ended, logged with its
/// exit code, and — for a crash or a hang — remembered for this boot and
/// these drivers, so it isn't started again until Windows restarts or a
/// driver changes. Nothing here lets a driver fault reach the service.
public sealed class GpuWorkerClient : IGpuPowerApi, IDisposable
{
    /// The app gives up on a request after 5 s; ADLX's start (even with its
    /// server restarted, #230) takes 2–3 s. A hang holds the control pipe
    /// for this long once, then the worker is off.
    public static readonly TimeSpan DefaultCallTimeout = TimeSpan.FromSeconds(8);

    private const string RetryNote = "They are tried again after Windows restarts or the graphics driver is updated.";

    private readonly Func<IGpuWorker> _start;
    private readonly string? _markerPath;
    private readonly DateTimeOffset _bootTime;
    private readonly string _drivers;
    private readonly TimeSpan _timeout;
    private readonly object _lock = new();
    private IGpuWorker? _worker;
    // The worker's output lines, read by its own thread — calls wait here with
    // their time limit, so nothing depends on the thread pool.
    private BlockingCollection<string?>? _lines;
    private string? _down;
    private int _nextId;

    public GpuWorkerClient(Func<IGpuWorker> start, string? markerPath, DateTimeOffset bootTime, string drivers, TimeSpan? timeout = null)
    {
        _start = start;
        _markerPath = markerPath;
        _bootTime = bootTime;
        _drivers = drivers;
        _timeout = timeout ?? DefaultCallTimeout;
    }

    public IReadOnlyList<GpuPowerAdapter> Adapters() =>
        Call(GpuWorkerProtocol.OpAdapters, null, 0, r => r!.AsArray().Select(n => GpuWorkerProtocol.FromJson(n!)).ToList());

    public int GetPowerLimit(string adapterId) => Call(GpuWorkerProtocol.OpGet, adapterId, 0, r => (int)r!);

    public void SetPowerLimit(string adapterId, int percent) => Call(GpuWorkerProtocol.OpSet, adapterId, percent, _ => 0);

    public bool IsAtFactory(string adapterId) => Call(GpuWorkerProtocol.OpIsAtFactory, adapterId, 0, r => (bool)r!);

    public void ResetToFactory(string adapterId) => Call(GpuWorkerProtocol.OpReset, adapterId, 0, _ => 0);

    /// Asks the worker to end its driver session and exit; the next call
    /// starts a new one. Never throws.
    public void EndSession()
    {
        lock (_lock)
        {
            if (_worker is not { } worker)
                return;
            var lines = _lines!;
            _worker = null;
            _lines = null;
            try
            {
                if (!worker.HasExited)
                {
                    worker.Input.WriteLine(GpuWorkerProtocol.Encode(new GpuWorkerRequest(++_nextId, GpuWorkerProtocol.OpEnd)));
                    worker.Input.Flush();
                    ReadLine(lines, TimeSpan.FromSeconds(5));
                    if (!worker.WaitForExit(TimeSpan.FromSeconds(3)))
                        worker.Kill();
                }
            }
            catch (Exception e)
            {
                SidecarLog.Log($"[rigstats-control] GPU power: the GPU worker didn't end cleanly: {e.Message}");
                TryKill(worker);
            }
            finally
            {
                worker.Dispose();
            }
        }
    }

    public void Dispose() => EndSession();

    private T Call<T>(string op, string? adapter, int value, Func<JsonNode?, T> parse)
    {
        lock (_lock)
        {
            var worker = Worker();
            var lines = _lines!;
            var request = new GpuWorkerRequest(++_nextId, op, adapter, value);
            string? line;
            var timedOut = false;
            try
            {
                worker.Input.WriteLine(GpuWorkerProtocol.Encode(request));
                worker.Input.Flush();
                (var answered, line) = ReadLine(lines, _timeout);
                timedOut = !answered;
            }
            catch (IOException)
            {
                line = null; // the pipe broke: the worker is gone
            }
            if (line is null)
                throw Down(worker, op, timedOut);

            GpuWorkerResponse response;
            try
            {
                response = GpuWorkerProtocol.DecodeResponse(line);
                if (response.Id != request.Id)
                    throw new JsonException($"answer {response.Id} to request {request.Id}");
            }
            catch (JsonException e)
            {
                // Not a driver crash, but the worker can't be trusted any more:
                // end it; the next call starts a new one.
                SidecarLog.Log($"[rigstats-control] GPU power: the GPU worker answered something unexpected to '{op}' ({e.Message}) — restarting it.");
                _worker = null;
                _lines = null;
                TryKill(worker);
                worker.Dispose();
                throw new InvalidOperationException("The GPU driver's worker answered something unexpected.");
            }
            if (!response.Ok)
                throw new InvalidOperationException(response.Error ?? "The GPU driver failed.");
            try
            {
                return parse(response.Result);
            }
            catch (Exception e) when (e is InvalidOperationException or InvalidCastException or NullReferenceException or FormatException)
            {
                throw new InvalidOperationException($"The GPU driver's worker answered '{op}' with something unreadable.");
            }
        }
    }

    /// The running worker, starting one when there is none — unless a crash
    /// this boot (this process's or, through the marker, an earlier one's)
    /// says not to.
    private IGpuWorker Worker()
    {
        if (_down is not null)
            throw new GpuDriverCrashedException(_down);
        if (_worker is { } running)
        {
            if (!running.HasExited)
                return running;
            throw Down(running, "idle", timedOut: false); // died between calls
        }
        if (GpuCrashGuard.CrashedBefore(ReadMarker(), _bootTime, _drivers) is { } earlier)
        {
            _down = earlier;
            SidecarLog.Log($"[rigstats-control] GPU power: the GPU worker crashed earlier this boot ({_drivers}) — not started again until a restart or a driver update (#370).");
            throw new GpuDriverCrashedException(_down);
        }
        try
        {
            _worker = _start();
            _lines = StartReader(_worker);
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] GPU power: the GPU worker could not be started: {e.Message}");
            throw new InvalidOperationException($"The GPU driver's worker could not be started: {e.Message}");
        }
        return _worker;
    }

    /// Ends a worker that crashed or hung, logs it and remembers it.
    private GpuDriverCrashedException Down(IGpuWorker worker, string op, bool timedOut)
    {
        _worker = null;
        _lines = null;
        if (timedOut)
            TryKill(worker);
        var exited = worker.WaitForExit(TimeSpan.FromSeconds(2));
        var code = exited ? worker.ExitCode : null;
        worker.Dispose();
        var what = timedOut
            ? $"did not answer within {_timeout.TotalSeconds:0} s"
            : $"crashed (exit code {FormatExitCode(code)})";
        _down = $"GPU power limits are off: the graphics driver {what}. {RetryNote}";
        SidecarLog.Log($"[rigstats-control] GPU power: the GPU worker {what} during '{op}' ({_drivers}) — GPU power limits off until a restart or a driver update; the service keeps running (#370).");
        WriteMarker(GpuCrashGuard.Marker(_bootTime, _drivers, _down));
        return new GpuDriverCrashedException(_down);
    }

    public static string FormatExitCode(int? code) => code is { } c ? $"0x{unchecked((uint)c):X8}" : "unknown";

    /// One thread per worker reads its output into a queue; null marks the
    /// end (the worker closed its output, exited or the stream broke).
    private static BlockingCollection<string?> StartReader(IGpuWorker worker)
    {
        var lines = new BlockingCollection<string?>();
        var reader = new Thread(() =>
        {
            try
            {
                while (worker.Output.ReadLine() is { } line)
                    lines.Add(line);
            }
            catch
            {
                // The stream broke: the worker is gone.
            }
            finally
            {
                lines.Add(null);
            }
        })
        {
            IsBackground = true,
            Name = "gpu-worker-output",
        };
        reader.Start();
        return lines;
    }

    /// The next line (null when the worker's output ended); `Answered` is
    /// false when nothing came within `timeout` — a hang, not a crash.
    private static (bool Answered, string? Line) ReadLine(BlockingCollection<string?> lines, TimeSpan timeout) =>
        lines.TryTake(out var line, timeout) ? (true, line) : (false, null);

    private static void TryKill(IGpuWorker worker)
    {
        try
        {
            worker.Kill();
        }
        catch
        {
            // Already gone.
        }
    }

    private string? ReadMarker()
    {
        try
        {
            return _markerPath is not null && File.Exists(_markerPath) ? File.ReadAllText(_markerPath) : null;
        }
        catch
        {
            return null;
        }
    }

    private void WriteMarker(string marker)
    {
        if (_markerPath is null)
            return;
        try
        {
            File.WriteAllText(_markerPath, marker);
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] GPU power: crash marker not written: {e.Message}");
        }
    }
}

/// The worker as a real child process: the service's own exe with
/// `--gpu-worker`, stdio redirected (UTF-8 without a byte-order mark, so the
/// first line is plain JSON), no window. It inherits the service's account.
/// Its log lines arrive on stderr and go into the service's log — two
/// processes appending to one file would collide.
public sealed class ProcessGpuWorker : IGpuWorker
{
    private const int MaxForwardedLines = 200;
    private static readonly Regex Timestamp = new(@"^\[\d{4}-\d{2}-\d{2} [^\]]+\] ", RegexOptions.Compiled);

    private readonly Process _process;
    private int _forwarded;

    private ProcessGpuWorker(Process process) => _process = process;

    public static ProcessGpuWorker Start(string exe, string arguments, Action<string>? log = null)
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
        var worker = new ProcessGpuWorker(process);
        var forward = log ?? (line => SidecarLog.Log($"[gpu-worker] {line}"));
        process.ErrorDataReceived += (_, e) =>
        {
            if (string.IsNullOrWhiteSpace(e.Data) || Interlocked.Increment(ref worker._forwarded) > MaxForwardedLines)
                return;
            forward(Timestamp.Replace(e.Data, "").TrimEnd());
        };
        if (!process.Start())
            throw new InvalidOperationException($"{exe} did not start.");
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

/// `rigstats-sensor.exe --gpu-worker`: loads the GPU driver libraries and
/// answers the service on stdin/stdout. Writes the diagnostics file the
/// service used to (`gpu-power.json`). Debug builds take `--crash-test` /
/// `--hang-test` to prove the service survives both.
public static class GpuWorkerHost
{
    public static void Run(string[] args, string programData)
    {
        var utf8 = new UTF8Encoding(false);
        using var input = new StreamReader(Console.OpenStandardInput(), utf8);
        using var output = new StreamWriter(Console.OpenStandardOutput(), utf8) { AutoFlush = true };
        using var adlx = AdlxGpuPower.TryLoad();
        using var nvml = NvmlGpuPower.TryLoad();
        try
        {
            Lighting.LightingDiagnostics.Write(Path.Combine(programData, "gpu-power.json"), new JsonObject
            {
                ["written_utc"] = DateTimeOffset.UtcNow.ToString("O"),
                ["adlx_installed"] = adlx is not null,
                ["nvml"] = nvml?.Diagnostics(),
                ["drivers"] = GpuCrashGuard.DriverFiles(),
            });
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] GPU power: gpu-power.json not written: {e.Message}");
        }
        Action? beforeFirstRequest = null;
#if DEBUG
        if (args.Contains("--crash-test"))
            beforeFirstRequest = () => Environment.FailFast("GPU worker crash test (#370)");
        else if (args.Contains("--hang-test"))
            beforeFirstRequest = () => Thread.Sleep(Timeout.Infinite);
#endif
        GpuWorkerLoop.Run(GpuPowerApis.Of(adlx, nvml), input, output, beforeFirstRequest);
    }
}
