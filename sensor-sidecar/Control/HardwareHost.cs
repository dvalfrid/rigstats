using LibreHardwareMonitor.Hardware;
using Microsoft.Extensions.Hosting;
using SensorSidecar; // UpdateVisitor, SensorReader

namespace SensorSidecar.Control;

/// The one thing allowed to touch the shared LHM `Computer` instance — every
/// telemetry read and every provider write goes through here, under the same
/// lock, so a fan write never interleaves with a Super I/O read. Extracted
/// from `SensorWorker`'s own `_computer`/`_sampleLock`/`GetFreshLine()`
/// (see `docs/control-architecture.md`, "HardwareHost"); the telemetry wire
/// format is unchanged, only the ownership moved.
public interface IHardwareHost
{
    /// The cached telemetry JSON line, re-sampling LHM only if the cache is
    /// stale (replaces `SensorWorker.GetFreshLine()`).
    Task<string> GetTelemetryLineAsync(CancellationToken ct);

    /// The same cached sample as <see cref="GetTelemetryLineAsync"/>, before
    /// serialization — for service-side consumers (`FanCurveLoop`) that must
    /// keep seeing fresh values even when no telemetry client is connected.
    Task<SensorPayload> GetSampleAsync(CancellationToken ct);

    /// Exclusive access to the raw `Computer` for a provider's `Capture`/
    /// `Apply`/`Verify`. Unused by `PowerPlanProvider` (power plan is an OS
    /// API, not LHM hardware).
    Task<T> WithHardwareLockAsync<T>(Func<IComputer, T> action, CancellationToken ct);

    /// Dumps the full sensor tree to disk for diagnostics.
    void WriteSensorTree(string path);
}

public sealed class HardwareHost : IHardwareHost, IHostedService, IDisposable
{
    private const long SampleMaxAgeMs = 900;

    private readonly Computer _computer;
    private readonly UpdateVisitor _visitor = new();
    private readonly Func<SensorPayload> _sample;
    private readonly System.Text.Json.JsonSerializerOptions _telemetryJsonOptions = new()
    {
        PropertyNamingPolicy = System.Text.Json.JsonNamingPolicy.SnakeCaseLower,
    };

    // A `lock`/`Monitor` can't be held across `await`, and provider `Apply`
    // calls are async — a semaphore is the async-safe equivalent of the
    // sample lock this replaces.
    private readonly SemaphoreSlim _hardwareLock = new(1, 1);

    // How long StopAsync waits for an in-flight sample/write before closing
    // the Computer anyway — an LHM call stuck in a driver must not hang
    // service shutdown.
    private static readonly TimeSpan CloseLockTimeout = TimeSpan.FromSeconds(5);

    private SensorPayload? _latestPayload;
    private string? _latestLine;
    private long _latestAtMs;

    // Set under _hardwareLock by StopAsync; every locked path checks it so a
    // straggler (a telemetry client mid-request, the identify spin, the
    // Program.cs release backstop) gets an exception, not a closed Computer.
    private bool _closed;

    public HardwareHost() : this(null) { }

    // Seam for tests (#208): `sample` replaces the LHM read, so the sample
    // rate can be pinned without hardware.
    internal HardwareHost(Func<SensorPayload>? sample)
    {
        _sample = sample ?? SampleLhm;
        _computer = new Computer
        {
            IsCpuEnabled = true,
            IsGpuEnabled = true,
            IsMemoryEnabled = true,
            IsMotherboardEnabled = true,
            IsStorageEnabled = true,
        };
    }

    // IHostedService: opening/closing the shared Computer is lifecycle-bound
    // to the whole service, not to any one worker — registered in
    // `Program.cs` as both the `IHardwareHost` singleton and a hosted
    // service resolving to that same instance.
    public Task StartAsync(CancellationToken cancellationToken)
    {
        _computer.Open();
        return Task.CompletedTask;
    }

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
            _computer.Close();
        }
        finally
        {
            if (acquired)
                _hardwareLock.Release();
        }
    }

    private void ThrowIfClosed()
    {
        if (_closed)
            throw new InvalidOperationException("Hardware host is stopped.");
    }

    /// Dumps the full sensor tree to disk for diagnostics — same shape as
    /// `SensorWorker`'s previous `WriteSensorTree`, kept here since it reads
    /// the same `_computer`/`_visitor`.
    public void WriteSensorTree(string path)
    {
        try
        {
            _computer.Accept(_visitor);
            var lines = new System.Text.StringBuilder();
            lines.AppendLine($"# sensor-tree — {DateTimeOffset.UtcNow:yyyy-MM-dd HH:mm:ss}Z");
            // Data sensors are in bytes since LHM 0.9.7; older dumps (MB/GB,
            // with SmallData) have no such line — SensorTreeLoader converts them.
            lines.AppendLine("# data-unit: bytes");
            foreach (var hw in _computer.Hardware)
            {
                lines.AppendLine($"HW  {hw.HardwareType,-20} id={hw.Identifier} name={hw.Name}");
                foreach (var s in hw.Sensors)
                    lines.AppendLine($"  S  {s.SensorType,-15} id={s.Identifier} name={s.Name} val={s.Value}");
                foreach (var sub in hw.SubHardware)
                {
                    lines.AppendLine($"  SUB {sub.HardwareType,-18} id={sub.Identifier} name={sub.Name}");
                    foreach (var s in sub.Sensors)
                        lines.AppendLine($"    S  {s.SensorType,-15} id={s.Identifier} name={s.Name} val={s.Value}");
                }
            }
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.WriteAllText(path, lines.ToString());
        }
        catch { }
    }

    public async Task<string> GetTelemetryLineAsync(CancellationToken ct)
    {
        await _hardwareLock.WaitAsync(ct);
        try
        {
            RefreshIfStale();
            return _latestLine!;
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

    // Caller holds _hardwareLock. One LHM read per SampleMaxAgeMs, however
    // many telemetry clients and service-side consumers ask for it.
    private void RefreshIfStale()
    {
        ThrowIfClosed();
        var now = Environment.TickCount64;
        if (_latestPayload is not null && now - _latestAtMs < SampleMaxAgeMs)
            return;
        _latestPayload = _sample();
        _latestLine = System.Text.Json.JsonSerializer.Serialize(_latestPayload, _telemetryJsonOptions);
        _latestAtMs = now;
    }

    private SensorPayload SampleLhm()
    {
        _computer.Accept(_visitor);
        return SensorReader.Extract(_computer);
    }

    public async Task<T> WithHardwareLockAsync<T>(Func<IComputer, T> action, CancellationToken ct)
    {
        await _hardwareLock.WaitAsync(ct);
        try
        {
            ThrowIfClosed();
            return action(_computer);
        }
        finally
        {
            _hardwareLock.Release();
        }
    }

    public void Dispose() => _hardwareLock.Dispose();
}
