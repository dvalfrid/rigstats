using System.Text.Json;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Workers;

/// The hardware worker is not running: it crashed, hung or could not start,
/// and the next try is after a pause (#374).
public sealed class HardwareWorkerDownException(string message) : InvalidOperationException(message);

/// The service's side of the hardware worker (#374): `ISensorHardware` over
/// the `--hardware-worker` child process. The worker starts with the first
/// call and stays. Every call has a time limit. A worker that crashes, hangs
/// or answers nonsense is ended and logged with its exit code — and, unlike
/// the GPU worker, started again: sensors and fans can't wait for a reboot.
/// The first restart is at once, then after 1, 2, 5, 15 and 60 s while it
/// keeps failing; a worker that ran for `StableAfter` starts the count over.
/// Meanwhile calls fail at once with `HardwareWorkerDownException`, so a
/// broken worker never holds up the telemetry pipe or the fan loop.
public sealed class HardwareWorkerClient : ISensorHardware
{
    /// A sample is normally well under a second; a storage device waking up
    /// can take a few. Silent this long, the worker is taken as hung.
    public static readonly TimeSpan DefaultCallTimeout = TimeSpan.FromSeconds(8);

    /// The first call of a new worker waits for LibreHardwareMonitor to open
    /// the hardware — a few seconds, longer right after boot.
    public static readonly TimeSpan DefaultStartTimeout = TimeSpan.FromSeconds(60);

    /// Pauses before each restart while the worker keeps failing.
    public static readonly IReadOnlyList<TimeSpan> DefaultBackoff =
    [
        TimeSpan.Zero, TimeSpan.FromSeconds(1), TimeSpan.FromSeconds(2),
        TimeSpan.FromSeconds(5), TimeSpan.FromSeconds(15), TimeSpan.FromSeconds(60),
    ];

    /// A worker that ran this long without failing resets the backoff.
    public static readonly TimeSpan StableAfter = TimeSpan.FromMinutes(1);

    /// Crashes while reading the same hardware before it is left out — until
    /// the service restarts, Windows' restart included; nothing is stored
    /// (#374, #338). Once can be bad luck; three is the
    /// hardware or its driver.
    public const int SkipAfter = 3;

    private readonly Func<IReadOnlyCollection<string>, IWorker> _start;
    private readonly TimeSpan _timeout;
    private readonly TimeSpan _startTimeout;
    private readonly IReadOnlyList<TimeSpan> _backoff;
    private readonly Func<long> _now;
    private readonly object _lock = new();
    private WorkerChannel? _channel;
    private long _startedAt;
    private int _failures;
    private string? _down;
    private long _retryAt;
    private int _nextId;
    // The hardware the running worker said it is reading, until its answer.
    private HardwareWorkerProgress? _reading;
    private readonly Dictionary<string, int> _crashes = [];
    // Left out, in the order it happened: id → (name, what happened).
    private readonly Dictionary<string, (string Name, string Detail)> _skipped = [];
    private volatile IReadOnlyList<Problem> _problems = [];

    /// `start` gets the hardware to leave out (`--skip`). `timeout` /
    /// `startTimeout` / `backoff` / `clock` (milliseconds, like
    /// Environment.TickCount64) are for tests.
    public HardwareWorkerClient(
        Func<IReadOnlyCollection<string>, IWorker> start,
        TimeSpan? timeout = null,
        TimeSpan? startTimeout = null,
        IReadOnlyList<TimeSpan>? backoff = null,
        Func<long>? clock = null)
    {
        _start = start;
        _timeout = timeout ?? DefaultCallTimeout;
        _startTimeout = startTimeout ?? DefaultStartTimeout;
        _backoff = backoff ?? DefaultBackoff;
        _now = clock ?? (() => Environment.TickCount64);
    }

    public SensorPayload Sample() =>
        Call(HardwareWorkerProtocol.OpSample, null, 0, HardwareWorkerProtocol.FromJson<SensorPayload>);

    public string SensorTree() =>
        Call(HardwareWorkerProtocol.OpSensorTree, null, 0, r => r!.GetValue<string>());

    public IReadOnlyList<FanHeaderInfo> FanHeaders() =>
        Call(HardwareWorkerProtocol.OpFanHeaders, null, 0, HardwareWorkerProtocol.FromJson<List<FanHeaderInfo>>);

    public void SetFan(string id, double duty) => Call(HardwareWorkerProtocol.OpFanSet, id, duty, _ => 0);

    public void SetFanDefault(string id) => Call(HardwareWorkerProtocol.OpFanDefault, id, 0, _ => 0);

    public FanReadback ReadFan(string id) =>
        Call(HardwareWorkerProtocol.OpFanRead, id, 0, HardwareWorkerProtocol.FromJson<FanReadback>);

    /// One problem per hardware left out — for the telemetry line, so the
    /// app's Status shows it (#374).
    public IReadOnlyList<Problem> Problems => _problems;

    /// Ends the running worker — the fan watchdog's action when the fan loop
    /// stalls (#374). Without the lock, which a hung call holds: killing the
    /// process ends that call (its output closes), and it records the failure
    /// and the restart as for any crash. Never throws.
    public void Restart(string reason)
    {
        if (Volatile.Read(ref _channel) is not { } channel)
            return;
        SidecarLog.Log($"[rigstats-sensor] Ending the hardware worker: {reason}.");
        channel.Kill();
    }

    /// Asks the worker to close the hardware and exit; never throws.
    public void Dispose()
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
                    channel.Send(HardwareWorkerProtocol.Encode(new HardwareWorkerRequest(++_nextId, HardwareWorkerProtocol.OpEnd)));
                    channel.Receive(TimeSpan.FromSeconds(5));
                    if (!channel.Worker.WaitForExit(TimeSpan.FromSeconds(3)))
                        channel.Kill();
                }
            }
            catch (Exception e)
            {
                SidecarLog.Log($"[rigstats-sensor] The hardware worker didn't end cleanly: {e.Message}");
                channel.Kill();
            }
            finally
            {
                channel.Dispose();
            }
        }
    }

    private T Call<T>(string op, string? header, double duty, Func<JsonNode?, T> parse)
    {
        lock (_lock)
        {
            var channel = Channel();
            var answer = Request(channel, op, header, duty, _timeout);
            if (!answer.Ok)
                throw new InvalidOperationException(answer.Error ?? "The hardware failed.");
            try
            {
                return parse(answer.Result);
            }
            catch (Exception e) when (e is JsonException or InvalidOperationException or InvalidCastException or NullReferenceException or FormatException)
            {
                throw new InvalidOperationException($"The hardware worker answered '{op}' with something unreadable.");
            }
        }
    }

    /// One request and its answer. A crash, a hang or a garbled answer ends
    /// the worker and throws `HardwareWorkerDownException`.
    private HardwareWorkerResponse Request(WorkerChannel channel, string op, string? header, double duty, TimeSpan limit)
    {
        var request = new HardwareWorkerRequest(++_nextId, op, header, duty);
        string? line = null;
        var hung = false;
        try
        {
            channel.Send(HardwareWorkerProtocol.Encode(request));
            // Progress lines (the hardware being read) come before the answer,
            // each within `limit`: hung means silent that long. Not one
            // deadline for the whole call — Environment.TickCount64 counts
            // while the PC sleeps, so a sample in flight across sleep looked
            // hung on wake, and three sleeps left the GPU out (#527).
            while (true)
            {
                (var answered, line) = channel.Receive(limit);
                hung = !answered;
                if (hung || line is null || Progress(line) is not { } progress)
                    break;
                _reading = progress;
            }
        }
        catch (IOException)
        {
            line = null; // the pipe broke: the worker is gone
        }
        if (line is null)
            throw Down(channel, hung ? $"did not answer '{op}' within {limit.TotalSeconds:0} s" : $"ended during '{op}'", hung);
        try
        {
            var response = HardwareWorkerProtocol.DecodeResponse(line);
            if (response.Id != request.Id)
                throw new JsonException($"answer {response.Id} to request {request.Id}");
            _reading = null;
            return response;
        }
        catch (JsonException e)
        {
            throw Down(channel, $"answered something unexpected to '{op}' ({e.Message})", hung: true);
        }
    }

    /// The running worker, starting one — and opening the hardware — when
    /// there is none, unless it is pausing after failures.
    private WorkerChannel Channel()
    {
        if (_channel is { } running)
        {
            if (!running.Worker.HasExited)
                return running;
            throw Down(running, "ended between calls", hung: false);
        }
        if (_down is not null && _now() < _retryAt)
            throw new HardwareWorkerDownException(_down);
        WorkerChannel channel;
        try
        {
            channel = new WorkerChannel(_start(_skipped.Keys.ToList()), "hardware-worker-output");
        }
        catch (Exception e)
        {
            throw Failed($"The hardware worker could not be started: {e.Message}");
        }
        _channel = channel;
        _startedAt = _now();
        var hello = Request(channel, HardwareWorkerProtocol.OpHello, null, 0, _startTimeout);
        if (!hello.Ok)
        {
            Down(channel, $"could not open the hardware ({hello.Error})", hung: true);
            throw new HardwareWorkerDownException(_down!);
        }
        SidecarLog.Log(_down is null
            ? "[rigstats-sensor] The hardware worker has opened the hardware."
            : "[rigstats-sensor] The hardware worker is running again.");
        _down = null;
        return channel;
    }

    /// Ends a worker that crashed or hung, logs it with its exit code and sets
    /// the pause before the next start.
    private HardwareWorkerDownException Down(WorkerChannel channel, string what, bool hung)
    {
        _channel = null;
        if (hung)
            channel.Kill();
        var exited = channel.Worker.WaitForExit(TimeSpan.FromSeconds(2));
        var code = exited ? channel.Worker.ExitCode : null;
        channel.Dispose();
        var ranFor = TimeSpan.FromMilliseconds(_now() - _startedAt);
        if (ranFor >= StableAfter)
            _failures = 0;
        var happened = $"The hardware worker {what} (exit code {GpuWorkerClientExitCode(code)}) after {ranFor.TotalSeconds:0} s";
        if (_reading is { } reading)
        {
            _reading = null;
            happened += $" while reading {reading.Name} ({reading.Updating})";
            if (Count(reading) && Skip(reading, happened))
                _failures = 0; // the cause is out: start again at once
        }
        return Failed(happened);
    }

    private bool Count(HardwareWorkerProgress reading)
    {
        var crashes = _crashes[reading.Updating] = _crashes.GetValueOrDefault(reading.Updating) + 1;
        return crashes >= SkipAfter;
    }

    /// Leaves `reading` out of every worker from now on, until the service
    /// restarts. True when it wasn't out already.
    private bool Skip(HardwareWorkerProgress reading, string happened)
    {
        if (!_skipped.TryAdd(reading.Updating, (reading.Name, happened)))
            return false;
        _problems = [.. _skipped.Select(s => HardwareProblems.Off(s.Key, s.Value.Name, s.Value.Detail))];
        SidecarLog.Log($"[rigstats-sensor] {reading.Name} ({reading.Updating}) crashed the hardware worker {SkipAfter} times — it is no longer read until the service or Windows restarts; the rest of the sensors keep working (#374).");
        return true;
    }

    /// A progress line, or null for anything else (an answer, or garbage the
    /// answer's decoding then reports).
    private static HardwareWorkerProgress? Progress(string line)
    {
        try
        {
            return HardwareWorkerProtocol.DecodeProgress(line);
        }
        catch (JsonException)
        {
            return null;
        }
    }

    private HardwareWorkerDownException Failed(string what)
    {
        var pause = _backoff[Math.Min(_failures, _backoff.Count - 1)];
        _failures++;
        _retryAt = _now() + (long)pause.TotalMilliseconds;
        _down = $"{what}; sensors and fan control pause until it is started again.";
        SidecarLog.Log($"[rigstats-sensor] {what} — starting it again {(pause == TimeSpan.Zero ? "now" : $"in {pause.TotalSeconds:0} s")}; the service keeps running (#374).");
        return new HardwareWorkerDownException(_down);
    }

    private static string GpuWorkerClientExitCode(int? code) => Providers.GpuWorkerClient.FormatExitCode(code);
}

/// What the user sees when hardware is no longer read because it kept
/// crashing the hardware worker (#374) — in the app's Status, from the
/// telemetry line. What happened, that the rest works, what to do.
public static class HardwareProblems
{
    public const string ReadMoreAnchor = "#gpu-sensors-missing-temp--clock--power--fan-or-sidecar-crashes";

    /// `id` is LHM's hardware id (`/gpu-nvidia/0`), `name` its name.
    public static Problem Off(string id, string name, string detail) => new()
    {
        Id = "hardware_off",
        Severity = ProblemSeverity.Attention,
        Title = $"{name} isn't read any more",
        Message = $"Reading {name} crashed RIGStats' sensor reader {HardwareWorkerClient.SkipAfter} times, so RIGStats stopped reading it. "
            + "Fans, the CPU, lighting and the other sensors keep working — only its values are missing. "
            + "A graphics card can do this when the PC switches it off to save power, or after a driver update: "
            + "install the latest driver. RIGStats reads it again after Windows or the RIGStats service restarts.",
        Actions = Actions(id),
        Detail = detail,
    };

    private static IReadOnlyList<ProblemAction> Actions(string id)
    {
        var actions = new List<ProblemAction>();
        if (id.StartsWith("/gpu-nvidia/", StringComparison.Ordinal))
            actions.Add(new ProblemAction { Kind = ProblemActionKind.OpenUrl, Label = "Get the NVIDIA driver", Target = Providers.GpuProblems.NvidiaDriverUrl });
        else if (id.StartsWith("/gpu-amd/", StringComparison.Ordinal))
            actions.Add(new ProblemAction { Kind = ProblemActionKind.OpenUrl, Label = "Get the AMD driver", Target = Providers.GpuProblems.AmdDriverUrl });
        actions.Add(ControlProblems.Diagnostics());
        actions.Add(ControlProblems.ReadMore(ReadMoreAnchor));
        return actions;
    }
}
