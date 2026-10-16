using System.Diagnostics;
using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using SensorSidecar.Control.Workers;

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
public sealed class GpuDriverCrashedException(string message) : DomainUnavailableException(message)
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
            + (e.Hung ? "RIGStats tries again in a few minutes." : "RIGStats tries again after a restart or a driver update.")
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

/// The service's side: `IGpuPowerApi` over the worker. The worker starts on
/// the first call and stays for the next. Every call has a time limit; a
/// worker that crashes, hangs or answers nonsense is ended, logged with its
/// exit code. A crash is remembered for this boot and these drivers (also
/// across a service restart, through the marker), so it isn't started again
/// until Windows restarts or a driver changes; a hang only for
/// `DefaultHangRetry` — right after boot a driver can just be slow. Nothing
/// here lets a driver fault reach the service. The process and its output
/// are in `Workers/` (#374); this is the GPU worker's policy.
public sealed class GpuWorkerClient : IGpuPowerApi, IDisposable
{
    /// The app gives up on a request after 5 s; ADLX's start (even with its
    /// server restarted, #230) takes 2–3 s. A hang holds the control pipe
    /// for this long once, then the worker is off.
    public static readonly TimeSpan DefaultCallTimeout = TimeSpan.FromSeconds(8);

    /// The first call of a new worker also starts it and ADLX — slow right
    /// after boot, while the driver and AMD's ADLX server come up.
    public static readonly TimeSpan DefaultStartTimeout = TimeSpan.FromSeconds(20);

    /// How long a hang turns the GPU power limits off before a new try.
    public static readonly TimeSpan DefaultHangRetry = TimeSpan.FromMinutes(10);

    private const string CrashRetryNote = "They are tried again after Windows restarts or the graphics driver is updated.";

    private readonly Func<IWorker> _start;
    private readonly string? _markerPath;
    private readonly DateTimeOffset _bootTime;
    private readonly string _drivers;
    private readonly TimeSpan _timeout;
    private readonly TimeSpan _startTimeout;
    private readonly TimeSpan _hangRetry;
    private readonly Func<long> _now;
    private readonly object _lock = new();
    private WorkerChannel? _channel;
    private string? _down;
    // When a hang's pause ends (Environment.TickCount64 ms); 0 = until restart.
    private long _downUntil;
    // The running worker has answered once: later calls get the short limit.
    private bool _answered;
    private int _nextId;

    /// `timeout` / `startTimeout` / `hangRetry` / `clock` (milliseconds,
    /// like Environment.TickCount64) are for tests.
    public GpuWorkerClient(
        Func<IWorker> start,
        string? markerPath,
        DateTimeOffset bootTime,
        string drivers,
        TimeSpan? timeout = null,
        TimeSpan? startTimeout = null,
        TimeSpan? hangRetry = null,
        Func<long>? clock = null)
    {
        _start = start;
        _markerPath = markerPath;
        _bootTime = bootTime;
        _drivers = drivers;
        _timeout = timeout ?? DefaultCallTimeout;
        _startTimeout = startTimeout ?? DefaultStartTimeout;
        _hangRetry = hangRetry ?? DefaultHangRetry;
        _now = clock ?? (() => Environment.TickCount64);
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
            if (_channel is not { } channel)
                return;
            _channel = null;
            try
            {
                if (!channel.Worker.HasExited)
                {
                    channel.Send(GpuWorkerProtocol.Encode(new GpuWorkerRequest(++_nextId, GpuWorkerProtocol.OpEnd)));
                    channel.Receive(TimeSpan.FromSeconds(5));
                    if (!channel.Worker.WaitForExit(TimeSpan.FromSeconds(3)))
                        channel.Worker.Kill();
                }
            }
            catch (Exception e)
            {
                SidecarLog.Log($"[rigstats-control] GPU power: the GPU worker didn't end cleanly: {e.Message}");
                channel.Kill();
            }
            finally
            {
                channel.Dispose();
            }
        }
    }

    public void Dispose() => EndSession();

    private T Call<T>(string op, string? adapter, int value, Func<JsonNode?, T> parse)
    {
        lock (_lock)
        {
            var channel = Channel();
            var request = new GpuWorkerRequest(++_nextId, op, adapter, value);
            var limit = _answered ? _timeout : _startTimeout;
            string? line;
            var timedOut = false;
            try
            {
                channel.Send(GpuWorkerProtocol.Encode(request));
                (var answered, line) = channel.Receive(limit);
                timedOut = !answered;
            }
            catch (IOException)
            {
                line = null; // the pipe broke: the worker is gone
            }
            if (line is null)
                throw Down(channel, op, timedOut, limit);

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
                _channel = null;
                channel.Kill();
                channel.Dispose();
                throw new InvalidOperationException("The GPU driver's worker answered something unexpected.");
            }
            _answered = true;
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
    private WorkerChannel Channel()
    {
        if (_down is not null)
        {
            if (_downUntil == 0 || _now() < _downUntil)
                throw new GpuDriverCrashedException(_down);
            SidecarLog.Log("[rigstats-control] GPU power: trying the GPU worker again after it stopped answering (#370).");
            _down = null;
            _downUntil = 0;
        }
        if (_channel is { } running)
        {
            if (!running.Worker.HasExited)
                return running;
            throw Down(running, "idle", timedOut: false, _timeout); // died between calls
        }
        if (GpuCrashGuard.CrashedBefore(ReadMarker(), _bootTime, _drivers) is { } earlier)
        {
            _down = earlier;
            SidecarLog.Log($"[rigstats-control] GPU power: the GPU worker crashed earlier this boot ({_drivers}) — not started again until a restart or a driver update (#370).");
            throw new GpuDriverCrashedException(_down);
        }
        try
        {
            _channel = new WorkerChannel(_start(), "gpu-worker-output");
            _answered = false;
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] GPU power: the GPU worker could not be started: {e.Message}");
            throw new InvalidOperationException($"The GPU driver's worker could not be started: {e.Message}");
        }
        return _channel;
    }

    /// Ends a worker that crashed or hung and logs it. A crash is remembered
    /// until a restart or a driver change (the marker), a hang for
    /// `_hangRetry` in this process only.
    private GpuDriverCrashedException Down(WorkerChannel channel, string op, bool timedOut, TimeSpan limit)
    {
        _channel = null;
        if (timedOut)
            channel.Kill();
        var exited = channel.Worker.WaitForExit(TimeSpan.FromSeconds(2));
        var code = exited ? channel.Worker.ExitCode : null;
        channel.Dispose();
        if (timedOut)
        {
            var minutes = Math.Max(1, (int)Math.Round(_hangRetry.TotalMinutes));
            _down = $"GPU power limits are off: the graphics driver did not answer within {limit.TotalSeconds:0} s. They are tried again in {minutes} minutes.";
            _downUntil = _now() + (long)_hangRetry.TotalMilliseconds;
            SidecarLog.Log($"[rigstats-control] GPU power: the GPU worker did not answer within {limit.TotalSeconds:0} s during '{op}' ({_drivers}) — GPU power limits off for {minutes} minutes; the service keeps running (#370).");
            return new GpuDriverCrashedException(_down);
        }
        var what = $"crashed (exit code {FormatExitCode(code)})";
        _down = $"GPU power limits are off: the graphics driver {what}. {CrashRetryNote}";
        _downUntil = 0;
        SidecarLog.Log($"[rigstats-control] GPU power: the GPU worker {what} during '{op}' ({_drivers}) — GPU power limits off until a restart or a driver update; the service keeps running (#370).");
        WriteMarker(GpuCrashGuard.Marker(_bootTime, _drivers, _down));
        return new GpuDriverCrashedException(_down);
    }

    public static string FormatExitCode(int? code) => code is { } c ? $"0x{unchecked((uint)c):X8}" : "unknown";

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
