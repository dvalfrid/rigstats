using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// ASUS Aura monitors and the ROG Aura Monitor Light Bar (#212) — the
/// current family (OpenRGB's `AsusMonitorController`, used as protocol
/// documentation only). Same 0xEC framing as the motherboard controller.
/// OpenRGB drives them in direct mode only, so breathing and spectrum cycle
/// are drawn by the service (<see cref="SoftwareEffectLoop"/>). Models seen
/// running their own effects — the effect command with mode 1 static, 2
/// breathing, 4 colour cycle, as ASUS DisplayWidget Center sends them — use
/// those instead: one write per change instead of 25 a second, and the
/// effect keeps running when the service stops. Older Aura monitors
/// (feature-report protocol: XG27AQ, XG279Q, ...) are out of scope.
///
/// The light bar also has a white desk lamp (#214): channel 1 of the same
/// effect command, static with two values — the 6500 K and the 2700 K
/// channel — instead of R, G, B; "get" 0x31 on channel 1 reads them back.
/// From ASUS DisplayWidget Center's `ScreenLightBarHid.dll` (read as
/// documentation, no code copied), verified on hardware.
public sealed class AsusMonitorDevice : ILightingDevice, ILampDevice, IDisposable
{
    private static readonly TimeSpan ReplyTimeout = TimeSpan.FromSeconds(1);

    private const byte ConfigLedCountIndex = 32;
    private const byte Effect = 0x35;
    private const byte GetEffect = 0x31;
    private const byte ReadFlag = 0x80;
    private const byte LampChannel = 0x01;
    private const byte ModeStatic = 0x01;
    private const byte ModeBreathing = 0x02;
    private const byte ModeCycle = 0x04;
    private const byte Direct = 0x40;
    private const byte DirectApplyChannel = 0x84; // apply (0x80) | direct channel 4
    private const byte DirectMode = 0xFF;

    private readonly object _lock = new();
    private readonly HidDeviceInfo _info;
    private readonly SoftwareEffectLoop _loop;
    private IHidDevice? _device;
    // What the lamp last showed while lit — switched back on to that.
    private (byte Cool, byte Warm)? _lampWhenOn;

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

    /// Models verified running their own effects (static, breathing, colour
    /// cycle); the others are drawn in direct mode by the service until
    /// someone confirms theirs.
    public static bool HasBuiltInEffects(ushort productId) => productId is 0x1BA3 or 0x1AC8;

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
        if (HasBuiltInEffects(_info.ProductId))
        {
            _loop.Stop();
            lock (_lock)
                Send([EffectReport(effect, red, green, blue)]);
            return;
        }
        lock (_lock)
            Send([Report(Effect, 0x00, 0x00, 0x00, DirectMode, 0x00, 0x00, 0x01)]);
        _loop.Start(effect, red, green, blue);
    }

    /// The device's own effect: `[0xEC, 0x35, channel 0, 0, 0, mode, 0, 0,
    /// 1, R, G, B]`. Off is static black.
    public static byte[] EffectReport(AuraEffect effect, byte red, byte green, byte blue) => effect switch
    {
        AuraEffect.Off => Report(Effect, 0x00, 0x00, 0x00, ModeStatic, 0x00, 0x00, 0x01, 0, 0, 0),
        AuraEffect.Breathing => Report(Effect, 0x00, 0x00, 0x00, ModeBreathing, 0x00, 0x00, 0x01, red, green, blue),
        AuraEffect.SpectrumCycle => Report(Effect, 0x00, 0x00, 0x00, ModeCycle, 0x00, 0x00, 0x01),
        _ => Report(Effect, 0x00, 0x00, 0x00, ModeStatic, 0x00, 0x00, 0x01, red, green, blue),
    };

    public JsonObject Diagnostics() => new()
    {
        ["vendor_id"] = $"{_info.VendorId:X4}",
        ["product_id"] = $"{_info.ProductId:X4}",
        ["interface"] = _info.Interface,
        ["product"] = _info.Product,
        ["leds"] = Zones[0].Leds,
        ["built_in_effects"] = HasBuiltInEffects(_info.ProductId),
        ["config_reply"] = _config,
    };

    /// Stops a running animation; the monitor keeps its last frame.
    public void Release() => _loop.Stop();

    public bool HasLamp => Kind == "light_bar";

    public void SetLamp(LampPart lamp)
    {
        if (!HasLamp)
            return;
        var values = Lamp.Channels(lamp);
        lock (_lock)
            WriteLamp(values);
    }

    public bool LampOn()
    {
        lock (_lock)
            return HasLamp && ReadLamp() is { } now && now != (0, 0);
    }

    public void SwitchLamp(bool on)
    {
        if (!HasLamp)
            return;
        lock (_lock)
        {
            if (on)
            {
                WriteLamp(_lampWhenOn ?? Lamp.Channels(new LampPart { On = true }));
                return;
            }
            if (ReadLamp() is { } now && now != (0, 0))
                _lampWhenOn = now;
            WriteLamp((0, 0));
        }
    }

    // Caller holds _lock. Writes, then reads back what the lamp took.
    private void WriteLamp((byte Cool, byte Warm) values)
    {
        Send([LampReport(values.Cool, values.Warm)]);
        var back = ReadLamp();
        if (back != values)
            throw new IOException($"{Name} did not take the lamp setting (read back {(back is { } b ? $"{b.Cool}/{b.Warm}" : "nothing")}).");
        if (values != (0, 0))
            _lampWhenOn = values;
    }

    // Caller holds _lock. The get command, then the same with the read flag
    // set, then the reply (as DisplayWidget Center does).
    private (byte Cool, byte Warm)? ReadLamp()
    {
        var device = _device ?? throw new ObjectDisposedException(Name);
        device.Write(Report(GetEffect, LampChannel));
        device.Write(Report(GetEffect | ReadFlag, LampChannel));
        for (var i = 0; i < 4; i++)
        {
            if (device.Read(ReplyTimeout) is not { } reply)
                return null;
            if (ParseLampReply(reply) is { } values)
                return values;
        }
        return null;
    }

    /// `[0xEC, 0x35, channel 1, 0, 0, static, 0, 0, 1, 6500K, 2700K]`.
    public static byte[] LampReport(byte cool, byte warm) =>
        Report(Effect, LampChannel, 0x00, 0x00, ModeStatic, 0x00, 0x00, 0x01, cool, warm);

    /// The lamp's two values from a get-effect reply on channel 1 (they
    /// sit one byte later than in the set command), or null for another reply.
    public static (byte Cool, byte Warm)? ParseLampReply(byte[] reply) =>
        reply.Length >= 12 && reply[0] == AuraUsb.ReportId && reply[1] == GetEffect && reply[2] == LampChannel
            ? (reply[10], reply[11])
            : null;

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
