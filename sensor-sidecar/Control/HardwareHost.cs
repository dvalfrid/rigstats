using Microsoft.Extensions.Hosting;

namespace SensorSidecar.Control;

/// The one way to the hardware LibreHardwareMonitor reads and the fan headers
/// it writes — every telemetry read and every fan write goes through here,
/// one at a time, so a fan write never interleaves with a Super I/O read. The
/// hardware itself runs in the hardware worker process (#374); this keeps the
/// shared sample and the telemetry line.
public interface IHardwareHost
{
    /// The cached telemetry JSON line, re-sampling only if the cache is
    /// stale. When a new sample can't be had (the worker is restarting), the
    /// last one is served with its age (`sample_age_ms`) — the app takes one
    /// older than 5 s as no data (#374).
    Task<string> GetTelemetryLineAsync(CancellationToken ct);

    /// The same cached sample, before serialization — for service-side
    /// consumers (`FanCurveLoop`) that must keep seeing fresh values even
    /// when no telemetry client is connected. Throws when no fresh sample can
    /// be had: a fan decision is never made on an old one.
    Task<SensorPayload> GetSampleAsync(CancellationToken ct);

    /// Dumps the full sensor tree to disk for diagnostics; never throws.
    void WriteSensorTree(string path);

    /// The full sensor tree as read now — the diagnostics export asks for it
    /// over the control pipe, next to the one written at start (#557).
    string SensorTree();

    IReadOnlyList<FanHeaderInfo> FanHeaders();

    void SetFan(string id, double duty);

    void SetFanDefault(string id);

    FanReadback ReadFan(string id);

    /// Ends a hung hardware worker; the next call starts a new one (the fan
    /// watchdog, #374).
    void RestartHardware(string reason);
}

public sealed class HardwareHost : IHardwareHost, IHostedService, IDisposable
{
    private const long SampleMaxAgeMs = 900;

    private readonly ISensorHardware _hardware;
    private readonly Problem? _startProblem;
    private readonly PeripheralStatusStore? _peripherals;
    private readonly ProfileStore? _profiles;
    private readonly Func<long> _now;

    /// How the telemetry line is written — public for the contract test
    /// shared with the Rust reader (sensor-sidecar.Tests/contract/).
    public static readonly System.Text.Json.JsonSerializerOptions TelemetryJsonOptions = new()
    {
        PropertyNamingPolicy = System.Text.Json.JsonNamingPolicy.SnakeCaseLower,
    };

    // A `lock`/`Monitor` can't be held across `await`; a semaphore can, and
    // `Wait` and `WaitAsync` share it.
    private readonly SemaphoreSlim _hardwareLock = new(1, 1);

    // How long StopAsync waits for an in-flight call before closing the
    // hardware anyway — a call stuck in a driver must not hang the stop.
    private static readonly TimeSpan CloseLockTimeout = TimeSpan.FromSeconds(5);

    // The last good sample, its line and when it was taken — one immutable
    // snapshot, so telemetry can read it without the lock (#374).
    private sealed record Latest(SensorPayload Payload, string Line, long AtMs);
    private volatile Latest? _latest;

    /// How long telemetry waits for the hardware before it serves the last
    /// sample instead: a hung or restarting worker holds the lock for up to
    /// its time limit, and the app gives up on a read after 1.2 s — it would
    /// reconnect over and over meanwhile (seen live, #374).
    internal static readonly TimeSpan TelemetryBusyWait = TimeSpan.FromMilliseconds(300);

    // Set under _hardwareLock by StopAsync; every locked path checks it so a
    // straggler (a telemetry client mid-request, the identify spin, the
    // Program.cs release backstop) gets an exception, not a closed worker.
    private bool _closed;

    /// `clock` (milliseconds, like Environment.TickCount64) is for tests.
    public HardwareHost(ISensorHardware hardware, PeripheralStatusStore? peripherals = null, ProfileStore? profiles = null,
        Func<long>? clock = null, Problem? startProblem = null)
    {
        _hardware = hardware;
        _startProblem = startProblem;
        _peripherals = peripherals;
        _profiles = profiles;
        _now = clock ?? (() => Environment.TickCount64);
    }

    // The worker starts with the first call (SensorWorker writes the sensor
    // tree at start).
    public Task StartAsync(CancellationToken cancellationToken) => Task.CompletedTask;

    public async Task StopAsync(CancellationToken cancellationToken)
    {
        // Close under the lock, so it can't race a sample or fan write that
        // is still running (#204).
        var acquired = await _hardwareLock.WaitAsync(CloseLockTimeout, CancellationToken.None);
        try
        {
            if (!acquired)
                SidecarLog.Log("[rigstats-sensor] Hardware still busy at shutdown; closing anyway.");
            _closed = true;
            _hardware.Dispose();
        }
        finally
        {
            if (acquired)
                _hardwareLock.Release();
        }
    }

    public string SensorTree() => Locked(_hardware.SensorTree);

    public void WriteSensorTree(string path)
    {
        try
        {
            var tree = SensorTree();
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.WriteAllText(path, tree);
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-sensor] Sensor tree not written: {e.Message}");
        }
    }

    /// The sample runs in the background: a telemetry client starts it (or
    /// joins the one running) and waits at most `TelemetryBusyWait`. A hung or
    /// restarting worker would otherwise hold the very client that asked —
    /// the app gives up on a read after 1.2 s and reconnects (seen live,
    /// #374). Until the first sample there is nothing older to send, so the
    /// first request waits for it.
    public async Task<string> GetTelemetryLineAsync(CancellationToken ct)
    {
        ThrowIfClosed();
        var refresh = StartRefresh();
        if (_latest is { } last)
        {
            if (!refresh.IsCompleted)
                await Task.WhenAny(refresh, Task.Delay(TelemetryBusyWait, ct));
            ct.ThrowIfCancellationRequested();
            ThrowIfClosed();
            var latest = _latest!;
            // Fresh, or the last one with its age — the app tells them apart.
            return refresh.IsCompletedSuccessfully && _now() - latest.AtMs < SampleMaxAgeMs ? latest.Line : Aged(latest);
        }
        await refresh.WaitAsync(ct);
        ThrowIfClosed();
        return _latest!.Line;
    }

    private readonly object _refreshGate = new();
    private Task _refresh = Task.CompletedTask;

    /// The sample running now, or a new one when the cached one is stale; a
    /// completed task when it is fresh. Its failure is the caller's to see
    /// (the first request) — later ones send the last sample instead.
    private Task StartRefresh()
    {
        lock (_refreshGate)
        {
            if (!_refresh.IsCompleted)
                return _refresh;
            if (_latest is { } latest && _now() - latest.AtMs < SampleMaxAgeMs)
                return Task.CompletedTask;
            _refresh = Task.Run(async () =>
            {
                await _hardwareLock.WaitAsync();
                try
                {
                    RefreshIfStale();
                }
                finally
                {
                    _hardwareLock.Release();
                }
            });
            // Observed here: a failure nobody awaits must not surface as an
            // unobserved task exception.
            _ = _refresh.ContinueWith(t => _ = t.Exception, TaskContinuationOptions.OnlyOnFaulted);
            return _refresh;
        }
    }

    public async Task<SensorPayload> GetSampleAsync(CancellationToken ct)
    {
        await _hardwareLock.WaitAsync(ct);
        try
        {
            RefreshIfStale();
            return _latest!.Payload;
        }
        finally
        {
            _hardwareLock.Release();
        }
    }

    public IReadOnlyList<FanHeaderInfo> FanHeaders() => Locked(_hardware.FanHeaders);

    public void SetFan(string id, double duty) => Locked(() =>
    {
        _hardware.SetFan(id, duty);
        return 0;
    });

    public void SetFanDefault(string id) => Locked(() =>
    {
        _hardware.SetFanDefault(id);
        return 0;
    });

    public FanReadback ReadFan(string id) => Locked(() => _hardware.ReadFan(id));

    // Not under the lock: the hung call holds it, and ending the worker is
    // what releases it.
    public void RestartHardware(string reason)
    {
        if (_hardware is Workers.HardwareWorkerClient worker)
            worker.Restart(reason);
    }

    private T Locked<T>(Func<T> action)
    {
        _hardwareLock.Wait();
        try
        {
            ThrowIfClosed();
            return action();
        }
        finally
        {
            _hardwareLock.Release();
        }
    }

    private void ThrowIfClosed()
    {
        if (_closed)
            throw new InvalidOperationException("Hardware host is stopped.");
    }

    // Caller holds _hardwareLock. One sample per SampleMaxAgeMs, however
    // many telemetry clients and service-side consumers ask for it.
    private void RefreshIfStale()
    {
        ThrowIfClosed();
        var now = _now();
        if (_latest is { } latest && now - latest.AtMs < SampleMaxAgeMs)
            return;
        var payload = _hardware.Sample() with
        {
            Peripherals = _peripherals?.Latest?.ToList(),
            HardwareProblems = Problems(),
            ActiveProfile = _profiles?.ActiveProfile,
        };
        _latest = new Latest(payload, Serialize(payload), now);
    }

    // With the problems as they are now: a hardware just left out is news
    // exactly while the samples are old.
    private string Aged(Latest last) =>
        Serialize(last.Payload with { SampleAgeMs = _now() - last.AtMs, HardwareProblems = Problems() });

    // The worker's problems, plus one found at start (PawnIO missing, #507).
    private List<Problem>? Problems()
    {
        List<Problem> all = [.. _hardware.Problems];
        if (_startProblem is { } start)
            all.Add(start);
        return all.Count > 0 ? all : null;
    }

    private static string Serialize(SensorPayload payload) =>
        System.Text.Json.JsonSerializer.Serialize(payload, TelemetryJsonOptions);

    public void Dispose() => _hardwareLock.Dispose();
}
