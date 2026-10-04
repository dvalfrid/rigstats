using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// ASUS headsets on the GearLink "pattern 2" protocol (#212) — first the
/// ROG Delta II through its 2.4 GHz dongle (`1AFA`), verified on hardware.
/// Not in OpenRGB; the protocol is ASUS GearLink's own command schema
/// (read as documentation, no code copied): reports with id 0xCC on the
/// vendor collection (usage page 0xFF00), framed `[command, key, index0,
/// index1, data…]` with command 0x12 = get, 0x51 = set; a reply repeats the
/// four header bytes. Lighting: set key 40 / get key 3 with `effectId,
/// brightness (0–100), R, G, B` — so unlike other lighting devices, every
/// write is verified by reading it back.
public sealed class AsusHeadsetDevice : ILightingDevice, IDisposable
{
    private static readonly TimeSpan ReplyTimeout = TimeSpan.FromMilliseconds(800);

    private const byte Get = 0x12;
    private const byte Set = 0x51;
    private const byte DeviceInfoKey = 0;
    private const byte LightingGetKey = 3;
    private const byte LightingSetKey = 40;
    private const ushort VendorPage = 0xFF00;
    public const byte FullBrightness = 100;

    private readonly object _lock = new();
    private readonly HidDeviceInfo _info;
    private readonly byte _reportId;
    private IHidDevice? _device;
    private string _lastLighting = "";

    public string Id { get; }
    public string Name { get; }
    public string Kind => "headset";
    public string Firmware { get; }
    public string? Blocked => null;
    public IReadOnlyList<AuraZone> Zones { get; } = [new AuraZone("lights", "Lights", Addressable: false, 0, 1)];

    private AsusHeadsetDevice(HidDeviceInfo info, IHidDevice device, byte reportId, string id, string name, string firmware)
    {
        _info = info;
        _device = device;
        _reportId = reportId;
        Id = id;
        Name = name;
        Firmware = firmware;
    }

    /// Known headsets by USB product id — each verified on hardware (the
    /// protocol comes from GearLink, not OpenRGB). Also the source of the
    /// supported-devices list (<see cref="LightingCatalog"/>).
    public static readonly IReadOnlyDictionary<ushort, string> Models = new Dictionary<ushort, string>
    {
        [0x1AFA] = "ROG Delta II",
    };

    public static string? Model(ushort productId) => Models.GetValueOrDefault(productId);

    /// Every headset that answers. A dongle whose headset is off or out of
    /// range goes to `silent`, to be asked again later.
    public static IReadOnlyList<AsusHeadsetDevice> Discover(IReadOnlyList<HidDeviceInfo> hid, List<HidDeviceInfo>? silent = null)
    {
        var found = new List<AsusHeadsetDevice>();
        foreach (var info in hid.Where(d => d.VendorId == AuraUsb.AsusVendorId && Model(d.ProductId) is not null
                     && d.UsagePage == VendorPage && d.OutputReportLength > 0))
        {
            var model = Model(info.ProductId)!;
            var reportId = info.OutputReportId ?? 0xCC;
            IHidDevice? device = null;
            try
            {
                device = Hid.Open(info);
                var reply = Request(device, info, reportId, Get, DeviceInfoKey, []);
                if (reply is null || !HeadsetConnected(reply))
                {
                    LightingLog.Discovery($"[rigstats-control] Lighting: {model} 0x{info.ProductId:X4} " +
                        (reply is null ? "did not answer." : "dongle answered without a headset (off or out of range)."));
                    device.Dispose();
                    silent?.Add(info);
                    continue;
                }
                var firmware = FirmwareText(reply);
                var nth = found.Count + 1;
                LightingLog.Discovery($"[rigstats-control] Lighting: {model} 0x{info.ProductId:X4} report 0x{reportId:X2}, firmware {firmware}.");
                found.Add(new AsusHeadsetDevice(info, device, reportId,
                    $"asus-headset-{info.ProductId:x4}-{nth}", nth == 1 ? model : $"{model} ({nth})", firmware));
            }
            catch (Exception e)
            {
                LightingLog.Discovery($"[rigstats-control] Lighting: {model} 0x{info.ProductId:X4} not usable: {e.Message}");
                device?.Dispose();
            }
        }
        return found;
    }

    /// The dongle answers deviceInfo by itself too; with the headset off or
    /// out of range the headset's version is all zeros (verified on the
    /// Delta II).
    public static bool HeadsetConnected(byte[] reply) =>
        reply.Length >= 9 && reply.AsSpan(5, 4).IndexOfAnyExcept((byte)0) >= 0;

    /// "headset 0.9.4, dongle 0.9.4" from the deviceInfo reply (two 4-byte
    /// versions after the header).
    public static string FirmwareText(byte[] reply)
    {
        if (reply.Length < 13)
            return "";
        // All four bytes as they are — their meaning isn't documented.
        static string Version(byte[] r, int at) => $"{r[at]}.{r[at + 1]}.{r[at + 2]}.{r[at + 3]}";
        return $"headset {Version(reply, 5)}, dongle {Version(reply, 9)}";
    }

    public void Apply(AuraEffect effect, byte red, byte green, byte blue)
    {
        var data = LightingData(effect, red, green, blue);
        lock (_lock)
        {
            Send(Frame(_reportId, _info.OutputReportLength, Set, LightingSetKey, data));
            ReadReply(); // the set acknowledgement
            // Read the lighting back: what the headset reports is what counts.
            Send(Frame(_reportId, _info.OutputReportLength, Get, LightingGetKey, []));
            var back = ReadReply();
            _lastLighting = back is null ? "" : Convert.ToHexString(back[..Math.Min(10, back.Length)]);
            if (!ReadBackMatches(back, _reportId, data))
                throw new IOException($"{Name} did not take the lighting (read back {(_lastLighting.Length > 0 ? _lastLighting : "nothing")}).");
        }
    }

    /// The headset keeps its last lighting.
    public void Release() { }

    public JsonObject Diagnostics() => new()
    {
        ["vendor_id"] = $"{_info.VendorId:X4}",
        ["product_id"] = $"{_info.ProductId:X4}",
        ["interface"] = _info.Interface,
        ["usage_page"] = $"{_info.UsagePage:X4}",
        ["report_id"] = $"{_reportId:X2}",
        ["firmware"] = Firmware,
        ["last_lighting_reply"] = _lastLighting,
    };

    // ── Pure helpers (unit-tested) ─────────────────────────────────────

    /// `effectId, brightness, R, G, B`: static 1, breathing 2, colour cycle
    /// 4 (GearLink's numbering); off is static at brightness 0. The colour
    /// arrives already scaled by the profile's brightness.
    public static byte[] LightingData(AuraEffect effect, byte red, byte green, byte blue) => effect switch
    {
        AuraEffect.Off => [0x01, 0, 0, 0, 0],
        AuraEffect.Breathing => [0x02, FullBrightness, red, green, blue],
        AuraEffect.SpectrumCycle => [0x04, FullBrightness, red, green, blue],
        _ => [0x01, FullBrightness, red, green, blue],
    };

    public static byte[] Frame(byte reportId, int length, byte command, byte key, byte[] data)
    {
        var report = new byte[Math.Max(length, 5 + data.Length)];
        report[0] = reportId;
        report[1] = command;
        report[2] = key;
        data.CopyTo(report, 5); // after index0, index1
        return report;
    }

    /// A get-lighting reply carries the five data bytes after the header.
    public static bool ReadBackMatches(byte[]? reply, byte reportId, byte[] data) =>
        reply is { Length: >= 10 } && reply[0] == reportId && reply[1] == Get && reply[2] == LightingGetKey
        && reply.AsSpan(5, data.Length).SequenceEqual(data);

    private static byte[]? Request(IHidDevice device, HidDeviceInfo info, byte reportId, byte command, byte key, byte[] data)
    {
        device.Write(Frame(reportId, info.OutputReportLength, command, key, data));
        return device.Read(ReplyTimeout);
    }

    // Caller holds _lock.
    private byte[]? ReadReply() => (_device ?? throw new ObjectDisposedException(Name)).Read(ReplyTimeout);

    // Caller holds _lock. Reopens once (the dongle re-enumerates when the
    // headset wakes or reconnects).
    private void Send(byte[] report)
    {
        try
        {
            (_device ?? throw new ObjectDisposedException(Name)).Write(report);
        }
        catch (Exception e) when (e is IOException or ObjectDisposedException or System.ComponentModel.Win32Exception)
        {
            _device?.Dispose();
            _device = Hid.Open(_info);
            _device.Write(report);
        }
    }

    public void Dispose()
    {
        lock (_lock)
        {
            _device?.Dispose();
            _device = null;
        }
    }
}
