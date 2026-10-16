using System.Text.RegularExpressions;
using SensorSidecar.Control.Lighting;

namespace SensorSidecar.Control;

/// A Logitech device's own battery reading: the percentage (null when it
/// reports only a coarse level) and whether it's on power.
public readonly record struct LogitechReading(int? Percent, bool Charging);

/// Battery and charging of Logitech devices on Bluetooth, in their own
/// protocol HID++ 2.0 (#349) — Windows' Bluetooth battery has no charging
/// state. Read-only and limited to documented requests (Logitech's HID++ 2.0
/// feature specs, as implemented by Solaar's `hidpp20.py`): Root
/// `getFeature` (feature index 0, function 0) for Unified Battery `0x1004`,
/// else Battery Status `0x1000`, then that feature's status — function 1 of
/// `0x1004`, function 0 of `0x1000`. Nothing else is ever sent; pinned by
/// `LogitechBatteryTests`.
public static class LogitechBattery
{
    public const ushort Vendor = 0x046D;

    /// The HID++ collection of a Logitech device on Bluetooth LE.
    public const ushort BluetoothUsagePage = 0xFF43;

    private const byte LongReport = 0x11;
    private const byte ReportLength = 20;

    /// A device connected directly (Bluetooth, not through a receiver);
    /// behind a receiver a device is addressed by its slot, 1–6 (#407).
    public const byte DirectDevice = 0xFF;

    /// Our software id in the low nibble of the function byte, so replies
    /// can be told from the device's own notifications (software id 0).
    private const byte SoftwareId = 0x0B;

    private const ushort UnifiedBattery = 0x1004;
    private const ushort BatteryStatusFeature = 0x1000;

    private static readonly TimeSpan ReplyTimeout = TimeSpan.FromSeconds(1);

    /// The Bluetooth address from a HID-over-GATT path
    /// (`…_dev_vid&02046d_pid&b025_rev&0015_de6768c18cf5&col02#…`), the same
    /// address Windows' Bluetooth node has — or null.
    public static string? Address(string path) =>
        Regex.Match(path, @"_([0-9a-f]{12})&col", RegexOptions.IgnoreCase) is { Success: true } m
            ? m.Groups[1].Value.ToLowerInvariant()
            : null;

    /// The HID++ collections of Logitech devices on Bluetooth LE.
    public static IEnumerable<HidDeviceInfo> Candidates(IEnumerable<HidDeviceInfo> hid) =>
        hid.Where(h => h.VendorId == Vendor && h.UsagePage == BluetoothUsagePage
            && h.Path.Contains("{00001812-0000-1000-8000-00805f9b34fb}", StringComparison.OrdinalIgnoreCase));

    /// One long HID++ request to the directly connected device.
    public static byte[] Request(byte featureIndex, byte function, params byte[] args) =>
        RequestTo(DirectDevice, featureIndex, function, args);

    /// One long HID++ request to `device` (`DirectDevice` or a slot).
    public static byte[] RequestTo(byte device, byte featureIndex, byte function, params byte[] args)
    {
        var report = new byte[ReportLength];
        report[0] = LongReport;
        report[1] = device;
        report[2] = featureIndex;
        report[3] = (byte)((function << 4) | SoftwareId);
        args.CopyTo(report, 4);
        return report;
    }

    /// The reply's parameters when it answers this request, else null (a
    /// notification or another program's reply). An error reply (HID++ 2.0
    /// `FF`, 1.0 `8F` — from a receiver for a device that is asleep or gone)
    /// for this request throws.
    public static byte[]? Payload(byte[] reply, byte featureIndex, byte function, byte device = DirectDevice)
    {
        var functionByte = (byte)((function << 4) | SoftwareId);
        if (reply.Length < 5 || (reply[1] != device && !(device == DirectDevice && reply[1] == 0x00)))
            return null;
        if ((reply[2] == 0xFF || reply[2] == 0x8F) && reply[3] == featureIndex && reply[4] == functionByte)
            throw new InvalidOperationException($"HID++ error {(reply.Length > 5 ? reply[5] : 0):X2}");
        return reply[0] == LongReport && reply[2] == featureIndex && reply[3] == functionByte ? reply[4..] : null;
    }

    /// Unified Battery `0x1004` status: state of charge %, level flags,
    /// charging status (0 discharging, 1 charging, 2 slow, 3 full, 4 error),
    /// external power. A 0 % state of charge means the device reports only
    /// the level flags — the Windows percentage stays.
    public static LogitechReading Unified(byte[] payload) =>
        new(payload[0] is > 0 and <= 100 ? payload[0] : null, payload[2] is 1 or 2 or 3);

    /// Battery Status `0x1000`: discharge level %, next level, status
    /// (0 discharging, 1 recharging, 2 almost full, 3 full, 4 slow recharge).
    public static LogitechReading Status(byte[] payload) =>
        new(payload[0] is > 0 and <= 100 ? payload[0] : null, payload[2] is 1 or 2 or 3 or 4);

    /// The feature's index on the device (Root `getFeature`), or null when
    /// it doesn't have it.
    public static byte? FeatureIndex(IHidDevice device, ushort feature, byte slot = DirectDevice) =>
        Ask(device, slot, 0x00, 0x00, (byte)(feature >> 8), (byte)feature) is [var index, ..] && index != 0 ? index : null;

    /// The battery of the device at `slot`, finding the feature first unless
    /// `known` gives it. Returns the feature used, for the caller to remember.
    public static (LogitechReading? Reading, (ushort Feature, byte Index)? Found) Read(IHidDevice device, (ushort Feature, byte Index)? known, byte slot = DirectDevice)
    {
        var found = known;
        if (found is null)
        {
            if (FeatureIndex(device, UnifiedBattery, slot) is { } unified)
                found = (UnifiedBattery, unified);
            else if (FeatureIndex(device, BatteryStatusFeature, slot) is { } status)
                found = (BatteryStatusFeature, status);
            else
                return (null, null);
        }
        var (feature, index) = found.Value;
        var reading = feature == UnifiedBattery
            ? Ask(device, slot, index, 0x01) is { Length: >= 3 } u ? Unified(u) : (LogitechReading?)null
            : Ask(device, slot, index, 0x00) is { Length: >= 3 } s ? Status(s) : null;
        return (reading, found);
    }

    /// The Bluetooth readings, now from the device itself: its percentage
    /// when it has one, and charging.
    public static IReadOnlyList<PeripheralStatus> Apply(IReadOnlyList<PeripheralStatus> bluetooth, IReadOnlyDictionary<string, LogitechReading> readings) =>
        [.. bluetooth.Select(p => p.Id.StartsWith("bt:", StringComparison.Ordinal) && readings.TryGetValue(p.Id[3..], out var r)
            ? p with { Battery = r.Percent ?? p.Battery, Charging = r.Charging }
            : p)];

    private static byte[]? Ask(IHidDevice device, byte slot, byte featureIndex, byte function, params byte[] args)
    {
        device.Write(RequestTo(slot, featureIndex, function, args));
        var deadline = DateTime.UtcNow + ReplyTimeout;
        while (DateTime.UtcNow < deadline)
        {
            if (device.Read(deadline - DateTime.UtcNow) is not { } reply)
                return null;
            if (Payload(reply, featureIndex, function, slot) is { } payload)
                return payload;
        }
        return null;
    }
}

/// Reads the Logitech devices among the connected Bluetooth ones each round;
/// remembers each one's battery feature so a round is one request. A device
/// that fails is logged once until it works again.
public sealed class LogitechBatteryReader
{
    private readonly Dictionary<string, (ushort Feature, byte Index)> _features = [];
    private readonly Dictionary<string, LogitechReading> _logged = [];
    private readonly HashSet<string> _failureLogged = [];

    public IReadOnlyDictionary<string, LogitechReading> Read(IReadOnlyCollection<string> connected)
    {
        var result = new Dictionary<string, LogitechReading>();
        if (connected.Count == 0)
            return result;
        foreach (var info in LogitechBattery.Candidates(Hid.Enumerate()))
        {
            if (LogitechBattery.Address(info.Path) is not { } address || !connected.Contains(address) || result.ContainsKey(address))
                continue;
            try
            {
                using var device = Hid.Open(info);
                var (reading, found) = LogitechBattery.Read(device, _features.TryGetValue(address, out var known) ? known : null);
                if (found is { } f)
                    _features[address] = f;
                if (reading is not { } r)
                    continue;
                result[address] = r;
                _failureLogged.Remove(address);
                if (!_logged.TryGetValue(address, out var before) || before.Charging != r.Charging)
                    SidecarLog.Log($"[rigstats-control] Battery: Logitech {info.ProductId:X4} ({address}) {(r.Percent is { } p ? $"{p} %" : "level only")}{(r.Charging ? ", charging" : "")} (HID++ 0x{found!.Value.Feature:X4}).");
                _logged[address] = r;
            }
            catch (Exception e)
            {
                _features.Remove(address);
                if (_failureLogged.Add(address))
                    SidecarLog.Log($"[rigstats-control] Battery: Logitech {info.ProductId:X4} ({address}) not read: {e.Message}");
            }
        }
        return result;
    }
}
