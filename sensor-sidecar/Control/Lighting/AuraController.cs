using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// One lighting device RIGStats can drive — the motherboard's controller
/// today; monitors and peripherals later (#212 and follow-ups). Aura Sync:
/// a profile's one effect goes to every device, each translating it into
/// its own protocol.
public interface ILightingDevice
{
    /// Stable for this device model, e.g. "aura-usb-19af".
    string Id { get; }
    string Name { get; }
    /// "motherboard", later "monitor", "keyboard", ...
    string Kind { get; }
    string Firmware { get; }
    IReadOnlyList<AuraZone> Zones { get; }

    /// Why another controller owns this device right now (e.g. Windows
    /// Dynamic Lighting), or null — a blocked device is skipped, not fought.
    string? Blocked { get; }

    /// One effect and colour on every zone. Throws when the device is gone.
    void Apply(AuraEffect effect, byte red, byte green, byte blue);

    /// Hands the device back to its own effect (service stop, release).
    void Release();

    /// Everything a test fixture or bug fix needs (ids, firmware, raw
    /// config) for the diagnostics export's `lighting-devices.json`.
    JsonObject Diagnostics();
}

/// The first known Aura USB controller on this machine, kept open for the
/// service's lifetime. Re-opens once if a write fails (USB reset, resume).
public sealed class AuraController : ILightingDevice, IDisposable
{
    private static readonly TimeSpan ReplyTimeout = TimeSpan.FromSeconds(1);

    private readonly object _lock = new();
    private readonly HidDeviceInfo _info;
    private IHidDevice? _device;

    public string Id => $"aura-usb-{_info.ProductId:x4}";
    public string Name { get; }
    public string Kind => "motherboard";
    public string Firmware { get; }
    public AuraFamily Family { get; }
    public string? Blocked => null;
    public IReadOnlyList<AuraZone> Zones { get; }

    private readonly byte[] _configTable;

    private AuraController(HidDeviceInfo info, IHidDevice device, AuraFamily family, string firmware, IReadOnlyList<AuraZone> zones, byte[] configTable)
    {
        _info = info;
        _configTable = configTable;
        _device = device;
        Family = family;
        Firmware = firmware;
        Zones = zones;
        Name = family == AuraFamily.Motherboard ? "ASUS Aura motherboard controller" : "ASUS Aura addressable controller";
    }

    /// Null with a user-facing reason when there is no usable controller.
    public static AuraController? TryOpen(out string reason)
    {
        IReadOnlyList<HidDeviceInfo> devices;
        try
        {
            devices = Hid.Enumerate();
        }
        catch (Exception e)
        {
            reason = "Lighting is unavailable: USB devices could not be listed.";
            SidecarLog.Log($"[rigstats-control] Aura: HID enumeration failed: {e.Message}");
            return null;
        }

        // A controller exposes several collections; the Aura one has the
        // vendor usage page (or, on older ones, 65-byte reports).
        var candidates = devices
            .Where(d => d.VendorId == AuraUsb.AsusVendorId && AuraUsb.Family(d.ProductId) is not null)
            .OrderByDescending(d => d.UsagePage == AuraUsb.AuraUsagePage)
            .Where(d => d.UsagePage == AuraUsb.AuraUsagePage || d.OutputReportLength == AuraUsb.ReportLength)
            .ToList();
        foreach (var info in candidates)
        {
            var family = AuraUsb.Family(info.ProductId)!.Value;
            IHidDevice? device = null;
            try
            {
                device = Hid.Open(info);
                device.Write(AuraUsb.FirmwareRequest());
                var firmware = device.Read(ReplyTimeout) is { } fw ? AuraUsb.ParseFirmware(fw) ?? "" : "";
                device.Write(AuraUsb.ConfigTableRequest());
                var table = device.Read(ReplyTimeout) is { } cfg ? AuraUsb.ParseConfigTable(cfg) : null;
                if (table is null)
                {
                    SidecarLog.Log($"[rigstats-control] Aura: 0x{info.ProductId:X4} gave no config table.");
                    device.Dispose();
                    continue;
                }
                var zones = AuraUsb.Zones(family, table);
                // The firmware and table are what a fixture needs to add or
                // fix a board — they land in the diagnostics ZIP via this log.
                SidecarLog.Log($"[rigstats-control] Aura: 0x{info.ProductId:X4} {family} firmware '{firmware}', " +
                    $"config {Convert.ToHexString(table)}, zones {string.Join(", ", zones.Select(z => $"{z.Id}({z.Leds})"))}.");
                reason = "";
                return new AuraController(info, device, family, firmware, zones, table);
            }
            catch (Exception e)
            {
                SidecarLog.Log($"[rigstats-control] Aura: 0x{info.ProductId:X4} not usable: {e.Message}");
                device?.Dispose();
            }
        }
        reason = candidates.Count == 0
            ? "No ASUS Aura USB controller found on this motherboard."
            : "The ASUS Aura controller did not answer.";
        return null;
    }

    public void Apply(AuraEffect effect, byte red, byte green, byte blue) =>
        Send(AuraUsb.SetEffect(Family, Zones, effect, red, green, blue));

    public JsonObject Diagnostics() => new()
    {
        ["vendor_id"] = $"{_info.VendorId:X4}",
        ["product_id"] = $"{_info.ProductId:X4}",
        ["interface"] = _info.Interface,
        ["family"] = Family.ToString(),
        ["firmware"] = Firmware,
        ["config_table"] = Convert.ToHexString(_configTable),
    };

    /// Nothing to hand back: the controller keeps the last effect until the
    /// next cold boot restores its saved one.
    public void Release() { }

    private void Send(IReadOnlyList<byte[]> reports)
    {
        lock (_lock)
        {
            try
            {
                Write(reports);
            }
            catch (Exception e) when (e is IOException or ObjectDisposedException or System.ComponentModel.Win32Exception)
            {
                // A USB reset or resume invalidates the handle — reopen once.
                _device?.Dispose();
                _device = Hid.Open(_info);
                Write(reports);
            }
        }
    }

    private void Write(IReadOnlyList<byte[]> reports)
    {
        var device = _device ?? throw new ObjectDisposedException(nameof(AuraController));
        foreach (var report in reports)
            device.Write(report);
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
