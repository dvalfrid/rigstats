using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// A SteelSeries Siberia headset's one RGB zone (#512), over its consumer
/// collection (interface 3, 16-byte output reports, report id 1).
///
/// The protocol comes from OpenRGB's SteelSeries Siberia controller (GPL-2,
/// `Controllers/SteelSeriesController/SteelSeriesSiberiaController`) — read
/// as documentation, no code copied. A colour is four messages, each a
/// report `01 00 <command> <length> <payload…>`: `95 80 BF`, `80 52 20`,
/// `83 r g b`, `93 03 80`. There is no off: off is black. Nothing is asked
/// of the headset. `Verified = false` until a user confirms.
public sealed class SteelSeriesHeadsetDevice : ILightingDevice, IDisposable
{
    public const ushort SteelSeriesVendorId = 0x1038;
    public const int Interface = 3;
    public const byte ReportId = 0x01;
    public const int ReportLength = 16;

    public sealed record Model(string Name, bool Verified = false);

    /// Every model, by product id, as OpenRGB's detector lists this controller.
    public static readonly IReadOnlyDictionary<ushort, Model> Models = new Dictionary<ushort, Model>
    {
        [0x1229] = new("SteelSeries Siberia 350"),
    };

    /// SteelSeries' own software — while it runs it owns the lights. Process
    /// names not yet seen on a machine.
    public static readonly IReadOnlyList<string> SteelSeriesSoftware = ["SteelSeriesGG", "SteelSeriesEngine3", "SteelSeriesEngine"];

    public static bool IsChannel(HidDeviceInfo h) =>
        h.VendorId == SteelSeriesVendorId && Models.ContainsKey(h.ProductId) && h.Interface == Interface
        && h.OutputReportLength == ReportLength;

    private readonly HidDeviceInfo _info;
    private readonly Model _model;
    private readonly IHidDevice _device;
    private readonly Func<string?> _ownSoftware;

    public SteelSeriesHeadsetDevice(HidDeviceInfo info, Model model, IHidDevice device, Func<string?>? ownSoftware = null)
    {
        _info = info;
        _model = model;
        _device = device;
        _ownSoftware = ownSoftware ?? (() => HyperXKeyboardDevice.Running(SteelSeriesSoftware, "SteelSeries GG"));
    }

    public string Id => $"steelseries-{_info.VendorId:x4}-{_info.ProductId:x4}";
    public string Name => _model.Name;
    public string Kind => "headset";
    public string Firmware => "";
    public IReadOnlyList<AuraZone> Zones { get; } = [new AuraZone("headset", "Headset", Addressable: false, EffectChannel: 0, Leds: 1)];
    public string? Blocked => _ownSoftware() is { } program ? $"{program} is running" : null;
    public string? Problem { get; private set; }
    public bool StaticOnly => true;

    public static IReadOnlyList<SteelSeriesHeadsetDevice> Discover(IReadOnlyList<HidDeviceInfo> hid,
        Func<HidDeviceInfo, IHidDevice>? open = null)
    {
        open ??= Hid.Open;
        var found = new List<SteelSeriesHeadsetDevice>();
        foreach (var info in hid.Where(IsChannel).OrderBy(h => h.Path, StringComparer.OrdinalIgnoreCase))
        {
            var model = Models[info.ProductId];
            try
            {
                found.Add(new SteelSeriesHeadsetDevice(info, model, open(info)));
                LightingLog.Discovery($"[rigstats-control] Lighting: {model.Name} {info.VendorId:X4}:{info.ProductId:X4} (SteelSeries protocol).");
            }
            catch (Exception e) when (e is IOException or System.ComponentModel.Win32Exception or UnauthorizedAccessException)
            {
                LightingLog.Discovery($"[rigstats-control] Lighting: {model.Name} couldn't be opened: {e.Message}");
            }
        }
        return found;
    }

    /// Never throws (not confirmed yet): a failure shows as `Problem`.
    /// Breathing and spectrum show as the colour.
    public void Apply(AuraEffect effect, byte red, byte green, byte blue)
    {
        try
        {
            var (r, g, b) = effect == AuraEffect.Off ? ((byte)0, (byte)0, (byte)0) : (red, green, blue);
            foreach (var report in ColourReports(r, g, b))
                _device.Write(report);
            Problem = null;
        }
        catch (Exception e) when (e is IOException or System.ComponentModel.Win32Exception or InvalidOperationException or OperationCanceledException)
        {
            Problem = e.Message;
            SidecarLog.Log($"[rigstats-control] Lighting on {Name} failed: {e.Message}");
        }
    }

    /// The headset keeps the last colour.
    public bool Release() => false;

    public JsonObject Diagnostics() => new()
    {
        ["vendor_id"] = $"{_info.VendorId:X4}",
        ["product_id"] = $"{_info.ProductId:X4}",
        ["interface"] = _info.Interface,
        ["product"] = _info.Product,
        ["protocol"] = "steelseries_siberia",
        ["verified"] = _model.Verified,
    };

    public void Dispose() => _device.Dispose();

    /// One message: `01 00 <command> <payload length> <payload…>`, 16 bytes.
    public static byte[] Message(byte command, params byte[] payload)
    {
        var report = new byte[ReportLength];
        report[0] = ReportId;
        report[2] = command;
        report[3] = (byte)payload.Length;
        payload.CopyTo(report, 4);
        return report;
    }

    /// The four messages that set a colour.
    public static IReadOnlyList<byte[]> ColourReports(byte red, byte green, byte blue) =>
    [
        Message(0x95, 0x80, 0xBF),
        Message(0x80, 0x52, 0x20),
        Message(0x83, red, green, blue),
        Message(0x93, 0x03, 0x80),
    ];
}
