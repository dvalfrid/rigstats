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

    private SensorPayload? _latestPayload;
    private string? _latestLine;
    private long _latestAtMs;

    // Set under _hardwareLock by StopAsync; every locked path checks it so a
    // straggler (a telemetry client mid-request, the identify spin, the
    // Program.cs release backstop) gets an exception, not a closed worker.
    private bool _closed;

    /// `clock` (milliseconds, like Environment.TickCount64) is for tests.
    public HardwareHost(ISensorHardware hardware, PeripheralStatusStore? peripherals = null, ProfileStore? profiles = null,
        Func<long>? clock = null)
    {
        _hardware = hardware;
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

    public void WriteSensorTree(string path)
    {
        try
        {
            var tree = Locked(_hardware.SensorTree);
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.WriteAllText(path, tree);
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-sensor] Sensor tree not written: {e.Message}");
        }
    }

    public async Task<string> GetTelemetryLineAsync(CancellationToken ct)
    {
        await _hardwareLock.WaitAsync(ct);
        try
        {
            try
            {
                RefreshIfStale();
                return _latestLine!;
            }
            catch (Exception) when (!_closed && _latestPayload is not null)
            {
                // No fresh sample (the worker is restarting): the last one,
                // with its age, so the app can tell it isn't live (#374).
                return Serialize(_latestPayload with { SampleAgeMs = _now() - _latestAtMs });
            }
        }
        finally
        {
            _hardwareLock.Release();
        }
    }

    public async Task<SensorPayload> GetSampleAsync(CancellationToken ct)
    {
        await _hardwareLock.WaitAsync(ct);
        try
        {
            RefreshIfStale();
            return _latestPayload!;
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
        if (_latestPayload is not null && now - _latestAtMs < SampleMaxAgeMs)
            return;
        _latestPayload = _hardware.Sample() with
        {
            Peripherals = _peripherals?.Latest?.ToList(),
            ActiveProfile = _profiles?.ActiveProfile,
        };
        _latestLine = Serialize(_latestPayload);
        _latestAtMs = now;
    }

    private static string Serialize(SensorPayload payload) =>
        System.Text.Json.JsonSerializer.Serialize(payload, TelemetryJsonOptions);

    public void Dispose() => _hardwareLock.Dispose();
}
