using SensorSidecar.Control.Lighting;

namespace SensorSidecar.Control;

/// The battery of a SteelSeries Arctis Nova Pro Wireless headset, from its
/// base station (#621). Read-only, one documented question: the status
/// request `06 B0`. Two open-source tools document it the same way, both
/// read as documentation, no code copied:
///
/// - HeadsetControl, `lib/devices/steelseries_arctis_nova_pro_wireless.hpp`
///   (GPL-3.0): product ids `12E0` / `12E5`, interface 4, request `06 B0`,
///   battery level 0–8 at byte 6, status at byte 15;
/// - Arctis Sound Manager, `devices/nova_pro_wireless.yaml` (GPL-3.0): the
///   reply starts with `06 B0`, `headset_battery_charge` at byte 6 (0–8),
///   `headset_power_status` at byte 15 (`01` offline, `02` charging over the
///   cable, `08` online), and the base station's 5 s start-up before it
///   takes a command (`init_sleep_length_ms`).
///
/// The first byte is the report id: 6. Interface 4 has two vendor
/// collections (usage pages FF00 and FFC0); the one whose output reports
/// carry id 6 takes the request (FFC0 on the tester's base station).
/// SteelSeries GG talks to the same base station; Windows hands every input
/// report to every open handle, so only a reply that starts with `06 B0` is
/// read — GG's other replies are skipped.
public static class SteelSeriesBaseStation
{
    public const ushort Vendor = 0x1038;
    public const int Interface = 4;
    public const byte ReportId = 0x06;
    public const byte StatusCommand = 0xB0;

    /// HeadsetControl's and Arctis Sound Manager's product ids.
    public static readonly IReadOnlyDictionary<ushort, string> Models = new Dictionary<ushort, string>
    {
        [0x12E0] = "SteelSeries Arctis Nova Pro Wireless",
        [0x12E5] = "SteelSeries Arctis Nova Pro Wireless X",
    };

    /// Seen working: SirBlacky21 on 1.53.0-rc.1, 50 % matching SteelSeries GG
    /// (#621). He named the non-X model; his diagnostics (with the product id)
    /// didn't arrive, so 0x12E0 rests on his wording.
    public static readonly IReadOnlySet<ushort> Verified = new HashSet<ushort> { 0x12E0 };

    /// The base station's start-up: no question before it has been there
    /// this long.
    public static readonly TimeSpan Settle = TimeSpan.FromSeconds(5);

    private static readonly TimeSpan ReplyTimeout = TimeSpan.FromSeconds(1);

    /// The collection that takes the status request, one per base station.
    public static IEnumerable<HidDeviceInfo> Stations(IEnumerable<HidDeviceInfo> hid) =>
        hid.Where(h => h.VendorId == Vendor && Models.ContainsKey(h.ProductId) && h.Interface == Interface
            && h.OutputReportId == ReportId);

    /// `06 B0`, zero-padded to the collection's output report length.
    public static byte[] StatusRequest(int length)
    {
        var report = new byte[Math.Max(length, 2)];
        report[0] = ReportId;
        report[1] = StatusCommand;
        return report;
    }

    /// Whether `reply` answers the status request.
    public static bool IsStatus(byte[] reply) =>
        reply.Length > 15 && reply[0] == ReportId && reply[1] == StatusCommand;

    /// The headset's battery from a status reply; null when the headset is
    /// off or out of range (status `01`) or the level is out of its 0–8
    /// range.
    public static BatteryStatus? Battery(byte[] reply)
    {
        if (!IsStatus(reply) || reply[15] == 0x01 || reply[6] > 8)
            return null;
        return new BatteryStatus((reply[6] * 100 + 4) / 8, reply[15] == 0x02);
    }

    /// One status question, the battery or null on no answer.
    public static BatteryStatus? Read(IHidDevice station, int length)
    {
        station.Write(StatusRequest(length));
        var deadline = DateTime.UtcNow + ReplyTimeout;
        while (DateTime.UtcNow < deadline)
        {
            if (station.Read(deadline - DateTime.UtcNow) is not { } reply)
                return null;
            if (IsStatus(reply))
                return Battery(reply);
        }
        return null;
    }
}

/// One base station's headset, as the battery monitor sees it.
public sealed class SteelSeriesBaseStationDevice(HidDeviceInfo station, string name) : IBatteryDevice
{
    public string Id => $"steelseries-{station.VendorId:x4}-{station.ProductId:x4}";
    public string Name => name;
    public string Kind => "headset";
    public bool HasBattery => true;
    public string Connection => Lighting.Connection.Radio;

    public BatteryStatus? ReadBattery()
    {
        using var hid = Hid.Open(station);
        return SteelSeriesBaseStation.Read(hid, station.OutputReportLength);
    }
}

/// The base stations plugged in now, each asked only once it has been there
/// for <see cref="SteelSeriesBaseStation.Settle"/>; found ones are logged
/// once.
public sealed class SteelSeriesBaseStations
{
    private readonly Dictionary<string, DateTime> _firstSeen = [];

    public IReadOnlyList<IBatteryDevice> Devices(IEnumerable<HidDeviceInfo> hid, DateTime now)
    {
        var result = new List<IBatteryDevice>();
        var present = new HashSet<string>();
        foreach (var station in SteelSeriesBaseStation.Stations(hid))
        {
            present.Add(station.Path);
            var name = SteelSeriesBaseStation.Models[station.ProductId];
            if (!_firstSeen.TryGetValue(station.Path, out var since))
            {
                _firstSeen[station.Path] = since = now;
                SidecarLog.Log($"[rigstats-control] Battery: {name} base station {station.ProductId:X4} found (usage page {station.UsagePage:X4}, not yet verified).");
            }
            if (now - since >= SteelSeriesBaseStation.Settle)
                result.Add(new SteelSeriesBaseStationDevice(station, name));
        }
        foreach (var gone in _firstSeen.Keys.Where(k => !present.Contains(k)).ToList())
            _firstSeen.Remove(gone);
        return result;
    }
}
