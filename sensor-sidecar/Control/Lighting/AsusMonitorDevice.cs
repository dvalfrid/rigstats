using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// ASUS Aura monitors and the ROG Aura Monitor Light Bar (#212) — the
/// current family (OpenRGB's `AsusMonitorController`, used as protocol
/// documentation only). Same 0xEC framing as the motherboard controller but
/// direct mode only: no built-in effects, so breathing and spectrum cycle
/// are drawn by the service (<see cref="SoftwareEffectLoop"/>). Older Aura
/// monitors (feature-report protocol: XG27AQ, XG279Q, ...) are out of scope.
public sealed class AsusMonitorDevice : ILightingDevice, IDisposable
{
    private static readonly TimeSpan ReplyTimeout = TimeSpan.FromSeconds(1);

    private const byte ConfigLedCountIndex = 32;
    private const byte Effect = 0x35;
    private const byte Direct = 0x40;
    private const byte DirectApplyChannel = 0x84; // apply (0x80) | direct channel 4
    private const byte DirectMode = 0xFF;

    private readonly object _lock = new();
    private readonly HidDeviceInfo _info;
    private readonly SoftwareEffectLoop _loop;
    private IHidDevice? _device;

    public string Id { get; }
    public string Name { get; }
    public string Kind { get; }
    public string Firmware => "";
    public string? Blocked => null;
    public IReadOnlyList<AuraZone> Zones { get; }

    private readonly string _config;

    private AsusMonitorDevice(HidDeviceInfo info, IHidDevice device, string id, string name, string kind, int leds, string config)
    {
        _info = info;
        _config = config;
        _device = device;
        Id = id;
        Name = name;
        Kind = kind;
        Zones = [new AuraZone("leds", kind == "light_bar" ? "Light bar" : "Monitor lighting", Addressable: true, 0, leds)];
        _loop = new SoftwareEffectLoop(name, Draw);
    }

    /// Known current-family devices by product id.
    public static (string Name, string Kind)? Model(ushort productId) => productId switch
    {
        0x1BA3 => ("ROG Strix XG27AQDMG", "monitor"),
        0x1BC9 => ("ROG Strix XG27ACDNG", "monitor"),
        0x1BB4 => ("ROG Strix XG27UCG", "monitor"),
        0x1B2B => ("ROG Swift PG32UCDM", "monitor"),
        0x1C9B => ("ROG Swift PG32UCDMR", "monitor"),
        0x1BCA => ("ROG Swift PG32UCDP", "monitor"),
        0x1AC8 => ("ROG Aura Monitor Light Bar", "light_bar"),
        _ => null,
    };

    /// Every connected device of this family. Two identical monitors are two
    /// devices ("…", "… (2)"), in HID path order.
    public static IReadOnlyList<AsusMonitorDevice> Discover(IReadOnlyList<HidDeviceInfo> hid)
    {
        var found = new List<AsusMonitorDevice>();
        var candidates = hid
            .Where(d => d.VendorId == AuraUsb.AsusVendorId && d.UsagePage == AuraUsb.AuraUsagePage && Model(d.ProductId) is not null)
            .OrderBy(d => d.ProductId)
            .ThenBy(d => d.Path, StringComparer.OrdinalIgnoreCase);
        foreach (var info in candidates)
        {
            var (model, kind) = Model(info.ProductId)!.Value;
            var nth = found.Count(f => f._info.ProductId == info.ProductId) + 1;
            IHidDevice? device = null;
            try
            {
                device = Hid.Open(info);
                device.Write(AuraUsb.ConfigTableRequest());
                var reply = device.Read(ReplyTimeout);
                var leds = reply is { Length: > ConfigLedCountIndex } ? reply[ConfigLedCountIndex] : 0;
                if (leds == 0)
                {
                    LightingLog.Discovery($"[rigstats-control] Lighting: {model} 0x{info.ProductId:X4} reported no LEDs.");
                    device.Dispose();
                    continue;
                }
                LightingLog.Discovery($"[rigstats-control] Lighting: {model} 0x{info.ProductId:X4}, {leds} LED(s), " +
                    $"config {(reply is null ? "-" : Convert.ToHexString(reply))}.");
                found.Add(new AsusMonitorDevice(info, device,
                    $"asus-monitor-{info.ProductId:x4}-{nth}",
                    nth == 1 ? model : $"{model} ({nth})",
                    kind, leds, reply is null ? "" : Convert.ToHexString(reply)));
            }
            catch (Exception e)
            {
                LightingLog.Discovery($"[rigstats-control] Lighting: {model} 0x{info.ProductId:X4} not usable: {e.Message}");
                device?.Dispose();
            }
        }
        return found;
    }

    public void Apply(AuraEffect effect, byte red, byte green, byte blue)
    {
        lock (_lock)
            Send([Report(Effect, 0x00, 0x00, 0x00, DirectMode, 0x00, 0x00, 0x01)]);
        _loop.Start(effect, red, green, blue);
    }

    public JsonObject Diagnostics() => new()
    {
        ["vendor_id"] = $"{_info.VendorId:X4}",
        ["product_id"] = $"{_info.ProductId:X4}",
        ["interface"] = _info.Interface,
        ["product"] = _info.Product,
        ["leds"] = Zones[0].Leds,
        ["config_reply"] = _config,
    };

    /// Stops a running animation; the monitor keeps its last frame.
    public void Release() => _loop.Stop();

    private void Draw(byte red, byte green, byte blue)
    {
        lock (_lock)
            Send([DirectFrame(Zones[0].Leds, red, green, blue)]);
    }

    /// One direct-mode frame: every LED the same colour.
    public static byte[] DirectFrame(int leds, byte red, byte green, byte blue)
    {
        var report = Report(Direct, DirectApplyChannel, 0x00, (byte)leds);
        for (var i = 0; i < leds && 5 + 3 * i + 2 < AuraUsb.ReportLength; i++)
        {
            report[5 + 3 * i] = red;
            report[5 + 3 * i + 1] = green;
            report[5 + 3 * i + 2] = blue;
        }
        return report;
    }

    // Caller holds _lock. Reopens once if the handle went stale.
    private void Send(IReadOnlyList<byte[]> reports)
    {
        try
        {
            Write(reports);
        }
        catch (Exception e) when (e is IOException or ObjectDisposedException or System.ComponentModel.Win32Exception)
        {
            _device?.Dispose();
            _device = Hid.Open(_info);
            Write(reports);
        }
    }

    private void Write(IReadOnlyList<byte[]> reports)
    {
        var device = _device ?? throw new ObjectDisposedException(Name);
        foreach (var report in reports)
            device.Write(report);
    }

    private static byte[] Report(params byte[] body)
    {
        var report = new byte[AuraUsb.ReportLength];
        report[0] = AuraUsb.ReportId;
        body.CopyTo(report, 1);
        return report;
    }

    public void Dispose()
    {
        _loop.Dispose();
        lock (_lock)
        {
            _device?.Dispose();
            _device = null;
        }
    }
}
