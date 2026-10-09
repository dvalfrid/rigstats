using Microsoft.Extensions.Hosting;
using SensorSidecar.Control.Lighting;

namespace SensorSidecar.Control;

/// One wireless device's battery in the telemetry (`peripherals`, #290).
public sealed record PeripheralStatus(string Id, string Name, string Kind, int Battery, bool Charging, string Connection);

/// The latest battery readings, written by `PeripheralBatteryMonitor` and
/// read by `HardwareHost` for every telemetry sample.
public sealed class PeripheralStatusStore
{
    private volatile IReadOnlyList<PeripheralStatus>? _latest;

    /// Null until the first round has run.
    public IReadOnlyList<PeripheralStatus>? Latest => _latest;

    public void Set(IReadOnlyList<PeripheralStatus> latest) => _latest = latest;
}

/// Asks the lighting devices that have a battery for it about once a minute.
/// Battery levels change slowly and each question goes over the radio, so
/// not every telemetry tick. Each round first lets the lighting provider look
/// for devices plugged in or switched (a keyboard moved from its receiver to
/// its cable is a new HID device, and nothing else may trigger a rescan). A
/// device that doesn't answer (asleep, off, or still in a receiver's paired
/// list after switching to its cable) keeps its last reading for
/// <see cref="StaleRounds"/> rounds, then drops off until it answers again.
public sealed class PeripheralBatteryMonitor(LightingProvider lighting, PeripheralStatusStore store) : BackgroundService
{
    internal static readonly TimeSpan FirstDelay = TimeSpan.FromSeconds(10);
    internal static readonly TimeSpan Interval = TimeSpan.FromSeconds(60);

    /// Rounds (≈ minutes) a silent device's last reading is kept.
    public const int StaleRounds = 10;

    private readonly Dictionary<string, PeripheralStatus> _lastKnown = [];
    private readonly Dictionary<string, int> _misses = [];
    private readonly HashSet<string> _failureLogged = [];

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        try
        {
            await Task.Delay(FirstDelay, stoppingToken);
            while (!stoppingToken.IsCancellationRequested)
            {
                lighting.Rescan();
                store.Set(ReadAll(lighting.Devices));
                await Task.Delay(Interval, stoppingToken);
            }
        }
        catch (OperationCanceledException) { }
    }

    /// One round over `devices`: fresh readings, else the last known one for
    /// a device still connected and silent for fewer than <see cref="StaleRounds"/> rounds. Each device is guarded on its own, so one
    /// failing can't hide the others.
    public IReadOnlyList<PeripheralStatus> ReadAll(IEnumerable<ILightingDevice> devices)
    {
        var result = new List<PeripheralStatus>();
        var present = new HashSet<string>();
        foreach (var device in devices)
        {
            if (device is not IBatteryDevice { HasBattery: true } battery)
                continue;
            present.Add(device.Id);
            var fresh = false;
            try
            {
                if (battery.ReadBattery() is { } status)
                {
                    fresh = true;
                    // Logged when first read and when charging starts or stops,
                    // not every minute.
                    if (!_lastKnown.TryGetValue(device.Id, out var before) || before.Charging != status.Charging)
                        SidecarLog.Log($"[rigstats-control] Battery: {device.Name} {status.Percent} %{(status.Charging ? ", charging" : "")}.");
                    _lastKnown[device.Id] = new PeripheralStatus(device.Id, device.Name, device.Kind, status.Percent, status.Charging, battery.Connection);
                    _failureLogged.Remove(device.Id);
                }
            }
            catch (Exception e)
            {
                if (_failureLogged.Add(device.Id))
                    SidecarLog.Log($"[rigstats-control] Battery: {device.Name} not read: {e.Message}");
            }
            var misses = fresh ? 0 : _misses.GetValueOrDefault(device.Id) + 1;
            _misses[device.Id] = misses;
            if (misses >= StaleRounds)
                _lastKnown.Remove(device.Id);
            if (_lastKnown.TryGetValue(device.Id, out var known))
                result.Add(known);
        }
        foreach (var gone in _lastKnown.Keys.Concat(_misses.Keys).Where(id => !present.Contains(id)).ToList())
        {
            _lastKnown.Remove(gone);
            _misses.Remove(gone);
        }
        return result;
    }
}
