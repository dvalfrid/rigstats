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

/// Asks the lighting devices that have a battery for it every
/// <see cref="ReadInterval"/> — each question is a couple of small reports
/// over the radio, so not every telemetry tick. Every <see cref="ScanInterval"/>
/// it lets the lighting provider look for devices plugged in or switched (a
/// keyboard moved from its receiver to its cable is a new HID device, and
/// nothing else may trigger a rescan; the check is a HID list comparison,
/// full discovery only on a change) and reads at once when the device list
/// changed. A device that doesn't answer (asleep, off, or still in a
/// receiver's paired list after switching to its cable) keeps its last
/// reading for <see cref="StaleRounds"/> reads (10 minutes), then drops off
/// until it answers again. Charging over a cable on a 2.4 GHz device shows
/// only in the battery reply, so it appears within one read interval — plus
/// however long the device takes to report it.
public sealed class PeripheralBatteryMonitor(LightingProvider lighting, PeripheralStatusStore store) : BackgroundService
{
    internal static readonly TimeSpan FirstDelay = TimeSpan.FromSeconds(10);
    internal static readonly TimeSpan ScanInterval = TimeSpan.FromSeconds(5);
    internal static readonly TimeSpan ReadInterval = TimeSpan.FromSeconds(20);

    /// Reads a silent device's last reading is kept (30 × 20 s = 10 min).
    public const int StaleRounds = 30;

    private readonly Dictionary<string, PeripheralStatus> _lastKnown = [];
    private readonly Dictionary<string, int> _misses = [];
    private readonly HashSet<string> _failureLogged = [];

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        try
        {
            await Task.Delay(FirstDelay, stoppingToken);
            var lastRead = DateTime.MinValue;
            IReadOnlyList<string> lastIds = [];
            while (!stoppingToken.IsCancellationRequested)
            {
                lighting.Rescan();
                var devices = lighting.Devices;
                var ids = devices.Select(d => d.Id).ToList();
                if (ShouldRead(lastRead, DateTime.UtcNow, lastIds, ids))
                {
                    store.Set(ReadAll(devices));
                    lastRead = DateTime.UtcNow;
                    lastIds = ids;
                }
                await Task.Delay(ScanInterval, stoppingToken);
            }
        }
        catch (OperationCanceledException) { }
    }

    /// Whether this scan tick reads the batteries: the read interval has
    /// passed, or the device list changed (plugged, unplugged, switched).
    public static bool ShouldRead(DateTime lastRead, DateTime now, IReadOnlyList<string> lastIds, IReadOnlyList<string> ids) =>
        now - lastRead >= ReadInterval || !ids.SequenceEqual(lastIds);

    /// One read over `devices`: fresh readings, else the last known one for
    /// a device still connected and silent for fewer than <see cref="StaleRounds"/> reads. Each device is guarded on its own, so one
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
