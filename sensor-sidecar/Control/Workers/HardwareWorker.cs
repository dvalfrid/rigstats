using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using LibreHardwareMonitor.Hardware;

namespace SensorSidecar.Control.Workers;

// LibreHardwareMonitor runs in a child process of the service,
// `rigstats-sensor.exe --hardware-worker` (#374). It loads vendor libraries
// (NVIDIA's NVAPI and NVML, AMD's ADL) for the GPU sensors, and a native
// fault in one ends the process it runs in — #338: NVML crashed the whole
// service whenever a laptop switched its NVIDIA GPU off. In the worker, a
// crash costs a restart of the worker; sensors, fans and lighting stay with
// the service. Fan writes go through LHM too, so they run here as well.

/// One request to the hardware worker; `Op` is one of the `HardwareWorkerProtocol.Op*`.
public sealed record HardwareWorkerRequest(int Id, string Op, string? Header = null, double Duty = 0);

/// The worker's answer: `Result` when `Ok`, else `Error` (an LHM exception —
/// not a crash).
public sealed record HardwareWorkerResponse(int Id, bool Ok, JsonNode? Result = null, string? Error = null);

/// Written before each hardware is read (`{"updating":"/gpu-nvidia/0","name":"…"}`),
/// ahead of the answer: when the worker dies mid-sample, the last one names
/// the hardware it was reading (#374).
public sealed record HardwareWorkerProgress(string Updating, string Name);

/// One JSON line per request and per answer, over the worker's stdin/stdout.
public static class HardwareWorkerProtocol
{
    /// Answered once the hardware is open — the first call of every worker.
    public const string OpHello = "hello";
    public const string OpSample = "sample";
    public const string OpSensorTree = "sensor_tree";
    public const string OpFanHeaders = "fan_headers";
    public const string OpFanSet = "fan_set";
    public const string OpFanDefault = "fan_default";
    public const string OpFanRead = "fan_read";
    public const string OpEnd = "end";

    /// Snake case, like the telemetry line, so a sample crosses unchanged.
    public static readonly JsonSerializerOptions Options = HardwareHost.TelemetryJsonOptions;

    public static string Encode(HardwareWorkerRequest request) => JsonSerializer.Serialize(request, Options);

    public static string Encode(HardwareWorkerResponse response) => JsonSerializer.Serialize(response, Options);

    public static string Encode(HardwareWorkerProgress progress) => JsonSerializer.Serialize(progress, Options);

    /// A progress line rather than an answer — null for an answer.
    public static HardwareWorkerProgress? DecodeProgress(string line) =>
        line.StartsWith("{\"updating\"", StringComparison.Ordinal)
            ? JsonSerializer.Deserialize<HardwareWorkerProgress>(line, Options)
            : null;

    /// The worker argument naming hardware to leave alone.
    public const string SkipArgument = "--skip";

    public static HardwareWorkerRequest DecodeRequest(string line) =>
        JsonSerializer.Deserialize<HardwareWorkerRequest>(line, Options) ?? throw new JsonException("empty request");

    public static HardwareWorkerResponse DecodeResponse(string line) =>
        JsonSerializer.Deserialize<HardwareWorkerResponse>(line, Options) ?? throw new JsonException("empty response");

    public static JsonNode? ToJson<T>(T value) => JsonSerializer.SerializeToNode(value, Options);

    public static T FromJson<T>(JsonNode? node) =>
        node.Deserialize<T>(Options) ?? throw new JsonException($"no {typeof(T).Name} in the answer");
}

/// The worker's side: answers requests from `hardware` until the service says
/// "end" or closes stdin (it went away). An LHM exception becomes an error
/// answer; only a native fault ends the process — the point of running it here.
public static class HardwareWorkerLoop
{
    /// `beforeSample` is the debug builds' crash/hang test hook.
    public static void Run(ISensorHardware hardware, TextReader input, TextWriter output, Action? beforeSample = null)
    {
        while (input.ReadLine() is { } line)
        {
            HardwareWorkerRequest request;
            try
            {
                request = HardwareWorkerProtocol.DecodeRequest(line);
            }
            catch (Exception e)
            {
                Write(output, new HardwareWorkerResponse(0, false, Error: $"Unreadable request: {e.Message}"));
                continue;
            }
            if (request.Op == HardwareWorkerProtocol.OpSample)
                beforeSample?.Invoke();
            Write(output, Handle(hardware, request));
            if (request.Op == HardwareWorkerProtocol.OpEnd)
                break;
        }
    }

    public static HardwareWorkerResponse Handle(ISensorHardware hardware, HardwareWorkerRequest request)
    {
        try
        {
            JsonNode? result = request.Op switch
            {
                HardwareWorkerProtocol.OpHello or HardwareWorkerProtocol.OpEnd => null,
                HardwareWorkerProtocol.OpSample => HardwareWorkerProtocol.ToJson(hardware.Sample()),
                HardwareWorkerProtocol.OpSensorTree => hardware.SensorTree(),
                HardwareWorkerProtocol.OpFanHeaders => HardwareWorkerProtocol.ToJson(hardware.FanHeaders()),
                HardwareWorkerProtocol.OpFanSet => Done(() => hardware.SetFan(Header(request), request.Duty)),
                HardwareWorkerProtocol.OpFanDefault => Done(() => hardware.SetFanDefault(Header(request))),
                HardwareWorkerProtocol.OpFanRead => HardwareWorkerProtocol.ToJson(hardware.ReadFan(Header(request))),
                _ => throw new InvalidOperationException($"Unknown request '{request.Op}'."),
            };
            return new HardwareWorkerResponse(request.Id, true, result);
        }
        catch (Exception e)
        {
            return new HardwareWorkerResponse(request.Id, false, Error: e.Message);
        }
    }

    private static void Write(TextWriter output, HardwareWorkerResponse response)
    {
        output.WriteLine(HardwareWorkerProtocol.Encode(response));
        output.Flush();
    }

    private static string Header(HardwareWorkerRequest request) =>
        request.Header ?? throw new InvalidOperationException($"'{request.Op}' needs a header.");

    private static JsonNode? Done(Action action)
    {
        action();
        return null;
    }
}

/// `rigstats-sensor.exe --hardware-worker`: opens LibreHardwareMonitor and
/// answers the service on stdin/stdout. Debug builds take
/// `--crash-after <seconds>` / `--hang-after <seconds>` to prove the service
/// survives both, and restarts the worker.
public static class HardwareWorkerHost
{
    public static void Run(string[] args)
    {
        var utf8 = new UTF8Encoding(false);
        using var input = new StreamReader(Console.OpenStandardInput(), utf8);
        using var output = new StreamWriter(Console.OpenStandardOutput(), utf8) { AutoFlush = true };
        var computer = new Computer
        {
            IsCpuEnabled = true,
            IsGpuEnabled = true,
            IsMemoryEnabled = true,
            IsMotherboardEnabled = true,
            IsStorageEnabled = true,
        };
        computer.Open();
        try
        {
            var skip = Skipped(args);
            var crashReading = CrashReading(args);
            using var hardware = new LhmHardware(computer, skip, h =>
            {
                output.WriteLine(HardwareWorkerProtocol.Encode(new HardwareWorkerProgress(h.Identifier.ToString(), h.Name)));
                if (crashReading is not null && h.Identifier.ToString().StartsWith(crashReading, StringComparison.Ordinal))
                    Environment.FailFast($"Hardware worker crash test while reading {h.Identifier} (#374)");
            });
            HardwareWorkerLoop.Run(hardware, input, output, TestHook(args));
        }
        finally
        {
            computer.Close();
        }
    }

    /// Debug builds: `--crash-reading /gpu-amd` crashes while reading the
    /// first hardware whose id starts so — to see it left out (#374).
    private static string? CrashReading(string[] args)
    {
#if DEBUG
        var at = Array.IndexOf(args, "--crash-reading");
        return at >= 0 && at + 1 < args.Length ? args[at + 1] : null;
#else
        _ = args;
        return null;
#endif
    }

    /// `--skip /gpu-nvidia/0,/gpu-amd/1`: hardware not to read.
    private static HashSet<string> Skipped(string[] args)
    {
        var at = Array.IndexOf(args, HardwareWorkerProtocol.SkipArgument);
        return at >= 0 && at + 1 < args.Length
            ? args[at + 1].Split(',', StringSplitOptions.RemoveEmptyEntries).ToHashSet(StringComparer.Ordinal)
            : [];
    }

    private static Action? TestHook(string[] args)
    {
#if DEBUG
        var started = Environment.TickCount64;
        if (Seconds(args, "--crash-after") is { } crash)
            return () =>
            {
                if (Environment.TickCount64 - started > crash * 1000)
                    Environment.FailFast("Hardware worker crash test (#374)");
            };
        if (Seconds(args, "--hang-after") is { } hang)
            return () =>
            {
                if (Environment.TickCount64 - started > hang * 1000)
                    Thread.Sleep(Timeout.Infinite);
            };
#else
        _ = args;
#endif
        return null;
    }

#if DEBUG
    private static int? Seconds(string[] args, string name)
    {
        var at = Array.IndexOf(args, name);
        return at >= 0 && at + 1 < args.Length && int.TryParse(args[at + 1], System.Globalization.CultureInfo.InvariantCulture, out var s) ? s : null;
    }
#endif
}
