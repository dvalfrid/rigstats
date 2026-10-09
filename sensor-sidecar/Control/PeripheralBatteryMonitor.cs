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
/// not every telemetry tick. A device that doesn't answer (asleep, off)
/// keeps its last reading while it is still connected.
public sealed class PeripheralBatteryMonitor(LightingProvider lighting, PeripheralStatusStore store) : BackgroundService
{
    internal static readonly TimeSpan FirstDelay = TimeSpan.FromSeconds(10);
    internal static readonly TimeSpan Interval = TimeSpan.FromSeconds(60);

    private readonly Dictionary<string, PeripheralStatus> _lastKnown = [];
    private readonly HashSet<string> _failureLogged = [];

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        try
        {
            await Task.Delay(FirstDelay, stoppingToken);
            while (!stoppingToken.IsCancellationRequested)
            {
                store.Set(ReadAll(lighting.Devices));
                await Task.Delay(Interval, stoppingToken);
            }
        }
        catch (OperationCanceledException) { }
    }

    /// One round over `devices`: fresh readings, else the last known one for
    /// a device still connected. Each device is guarded on its own, so one
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
            try
            {
                if (battery.ReadBattery() is { } status)
                {
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
            if (_lastKnown.TryGetValue(device.Id, out var known))
                result.Add(known);
        }
        foreach (var gone in _lastKnown.Keys.Where(id => !present.Contains(id)).ToList())
            _lastKnown.Remove(gone);
        return result;
    }
}
