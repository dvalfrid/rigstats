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
    /// can take a few. Past this the worker is taken as hung.
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

    private readonly Func<IWorker> _start;
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

    /// `timeout` / `startTimeout` / `backoff` / `clock` (milliseconds, like
    /// Environment.TickCount64) are for tests.
    public HardwareWorkerClient(
        Func<IWorker> start,
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
        string? line;
        var hung = false;
        try
        {
            channel.Send(HardwareWorkerProtocol.Encode(request));
            (var answered, line) = channel.Receive(limit);
            hung = !answered;
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
            return response.Id == request.Id
                ? response
                : throw new JsonException($"answer {response.Id} to request {request.Id}");
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
            channel = new WorkerChannel(_start(), "hardware-worker-output");
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
        return Failed($"The hardware worker {what} (exit code {GpuWorkerClientExitCode(code)}) after {ranFor.TotalSeconds:0} s");
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
