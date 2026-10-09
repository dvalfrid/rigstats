using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// One ASUS keyboard model of the Aura "TUF keyboard" protocol family.
/// `PerKey` keyboards carry a 0x02 marker before the colour; TUF K1/K5 don't.
/// `BrightnessMax` is the keyboard's own scale; `Speed` the effect speed
/// that matches the other devices' pace (each model has its own scale).
/// `Verified`: seen working on real hardware; otherwise from OpenRGB, or
/// from ASUS Gear Link's device modules when `GearLink` (#240).
public sealed record KeyboardModel(string Name, bool PerKey, int BrightnessMax, byte Speed, bool Receiver = false,
    bool Verified = false, bool GearLink = false);

/// ASUS keyboards of the Aura "TUF keyboard" protocol family (#213) — the
/// family of the original ROG Azoth in OpenRGB (protocol documentation
/// only): ROG Azoth, Falchion, Strix Flare / Flare II, Strix Scope / RX /
/// NX / II / II 96 and TUF Gaming K1/K3/K5/K7, plus anything paired to the
/// ROG Omni receiver — the ROG Azoth X there is verified on hardware — and
/// newer models verified here beyond OpenRGB (ROG Falchion Ace HFX). Each
/// model's vendor collection (usage page 0xFF00) takes the effect command;
/// on the receiver every paired device has its own channel (0xFF00–0xFF02,
/// own report id) and the keyboard's is the one that answers "get layout"
/// with a layout — found, not hard-coded. The keyboard's own effects are
/// used (static, breathing, colour cycle), so nothing is animated over a
/// radio. Effects are not saved to the keyboard (no 0x50 0x55): its own
/// effect returns after it sleeps or reconnects, and the service re-applies
/// the profile.
///
/// Not covered: the ROG Claymore (a different command layout, 2018) and the
/// Strix Scope TKL family (direct per-key mode only, model key maps).
public sealed class AsusKeyboardDevice : ILightingDevice, IBatteryDevice, IDisposable
{
    private static readonly TimeSpan ReplyTimeout = TimeSpan.FromMilliseconds(500);

    private const byte Get = 0x12;
    private const byte GetVersion = 0x00;
    private const byte GetLayout = 0x12;
    private const byte GetPower = 0x01;
    private const byte Set = 0x51;
    private const byte SetEffect = 0x2C;
    private const byte PerKeyMarker = 0x02;
    private const ushort VendorPage = 0xFF00;

    private readonly object _lock = new();
    private readonly HidDeviceInfo _info;
    private readonly KeyboardModel _model;
    private readonly byte _reportId;
    private readonly string _version;
    private readonly string _layout;
    private IHidDevice? _device;

    public string Id { get; }
    public string Name { get; }
    public string Kind => "keyboard";

    /// The keyboard's own USB product id when connected directly, or null
    /// behind a receiver. A keyboard driven here may also expose a HID
    /// LampArray collection (the Falchion Ace HFX does); that one is left
    /// out of discovery, so two paths don't fight over the same keys.
    public ushort? DirectProductId => _model.Receiver ? null : _info.ProductId;
    public string Firmware => "";
    public string? Blocked => null;
    public IReadOnlyList<AuraZone> Zones { get; } = [new AuraZone("keys", "Keys", Addressable: true, 0, 1)];

    private AsusKeyboardDevice(HidDeviceInfo info, IHidDevice device, KeyboardModel model, byte reportId,
        string id, string name, string version, string layout, bool hasBattery)
    {
        HasBattery = hasBattery;
        _info = info;
        _device = device;
        _model = model;
        _reportId = reportId;
        _version = version;
        _layout = layout;
        Id = id;
        Name = name;
    }

    // Effect speed per model family (OpenRGB): the Azoth group runs 255
    // slow … 0 fast, default 30 (verified on the Azoth X); Flare/K3/K7 run
    // 15 … 0, default 8; the K1 0 … 2, default 1.
    private static KeyboardModel Azoth(string name) => new(name, PerKey: true, BrightnessMax: 4, Speed: 30);
    private static KeyboardModel Flare(string name) => new(name, PerKey: true, BrightnessMax: 4, Speed: 8);

    // From ASUS Gear Link's device modules: every keyboard it supports
    // builds its effect as 0x51 0x2C <effect> with speed, brightness 0–100
    // (default 50), flags and colours — the Azoth X's protocol and scale.
    // Some ids of one model are its 2.4 GHz dongle or Bluetooth; which is
    // which isn't known, so they share a name.
    private static KeyboardModel GearLink(string name) =>
        new(name, PerKey: true, BrightnessMax: 100, Speed: 30, GearLink: true);

    /// Known models by USB product id. Also the source of the
    /// supported-devices list (<see cref="LightingCatalog"/>).
    public static readonly IReadOnlyDictionary<ushort, KeyboardModel> Models = new Dictionary<ushort, KeyboardModel>
    {
        // Verified on hardware: the Azoth X behind the receiver uses 0–100.
        [0x1ACE] = new("Keyboard via ROG Omni receiver", PerKey: true, BrightnessMax: 100, Speed: 30, Receiver: true, Verified: true),
        // The Azoth X by cable (verified): same keyboard, same 0–100 scale.
        [0x1C24] = new("ROG Azoth X", PerKey: true, BrightnessMax: 100, Speed: 30, Verified: true),
        // Verified on hardware: same layout reply as the Azoth X, 0–100 scale.
        // It also exposes a LampArray collection, left alone (see DirectProductId).
        [0x1B7E] = new("ROG Falchion Ace HFX", PerKey: true, BrightnessMax: 100, Speed: 30, Verified: true),
        // Verified on hardware: 0–100, not OpenRGB's 0–4 (at 4 it was very dim).
        [0x1A83] = new("ROG Azoth", PerKey: true, BrightnessMax: 100, Speed: 30, Verified: true),
        // The same keyboard over its dongle — the Azoth X keeps one scale on
        // cable and dongle, so this one is assumed 0–100 too (not verified).
        [0x1A85] = new("ROG Azoth (2.4 GHz)", PerKey: true, BrightnessMax: 100, Speed: 30),
        [0x193C] = Azoth("ROG Falchion"),
        [0x193E] = Azoth("ROG Falchion (wireless)"),
        [0x19FE] = Azoth("ROG Strix Flare II"),
        [0x19FC] = Azoth("ROG Strix Flare II Animate"),
        [0x18F8] = Azoth("ROG Strix Scope"),
        [0x1951] = Azoth("ROG Strix Scope RX"),
        [0x1B12] = Azoth("ROG Strix Scope RX EVA-02 Edition"),
        [0x19F6] = Azoth("ROG Strix Scope NX Wireless Deluxe"),
        [0x19F8] = Azoth("ROG Strix Scope NX Wireless Deluxe (2.4 GHz)"),
        [0x1AB3] = Azoth("ROG Strix Scope II"),
        // In Gear Link with a 0–100 slider, not OpenRGB's 0–4 (the Azoth's
        // OpenRGB 0–4 also turned out to be 0–100 on hardware).
        [0x1AB5] = GearLink("ROG Strix Scope II RX"),
        [0x1AB6] = GearLink("ROG Strix Scope II RX"),
        [0x1AAE] = GearLink("ROG Strix Scope II 96 Wireless"),
        [0x1AAF] = GearLink("ROG Strix Scope II 96 Wireless"),
        [0x1AB0] = GearLink("ROG Strix Scope II 96 Wireless"),
        [0x1B78] = Azoth("ROG Strix Scope II 96 RX Wireless"),
        [0x1C25] = GearLink("ROG Azoth X"),
        [0x1B7F] = GearLink("ROG Falchion Ace HFX"),
        [0x1D1D] = GearLink("ROG Falchion Ace HFX ZywOo Edition"),
        [0x1C7D] = GearLink("ROG Falchion Ace 75 HE"),
        [0x1B3F] = GearLink("ROG Azoth Extreme"),
        [0x1B40] = GearLink("ROG Azoth Extreme"),
        [0x1B42] = GearLink("ROG Azoth Extreme"),
        [0x1CEF] = GearLink("ROG Azoth Extreme Special Edition"),
        [0x1CF0] = GearLink("ROG Azoth Extreme Special Edition"),
        [0x1CF1] = GearLink("ROG Azoth Extreme Special Edition"),
        [0x1DA2] = GearLink("ROG Azoth Extreme Edition 20"),
        [0x1DA3] = GearLink("ROG Azoth Extreme Edition 20"),
        [0x1DA4] = GearLink("ROG Azoth Extreme Edition 20"),
        [0x1C10] = GearLink("ROG Azoth 96 HE / Lite"),
        [0x1C11] = GearLink("ROG Azoth 96 HE / Lite"),
        [0x1C12] = GearLink("ROG Azoth 96 HE / Lite"),
        [0x1C2F] = GearLink("ROG Falcata"),
        [0x1C31] = GearLink("ROG Falcata"),
        [0x1CE8] = GearLink("ROG Strix Morph 96 Wireless"),
        [0x1CE9] = GearLink("ROG Strix Morph 96 Wireless"),
        [0x1CEA] = GearLink("ROG Strix Morph 96 Wireless"),
        [0x1E0A] = GearLink("ROG Strix Morph 96 X Wireless"),
        [0x1E0B] = GearLink("ROG Strix Morph 96 X Wireless"),
        [0x1E0C] = GearLink("ROG Strix Morph 96 X Wireless"),
        [0x1E5B] = GearLink("ROG Strix Morph 96 Wireless (PBZ)"),
        [0x1E5C] = GearLink("ROG Strix Morph 96 Wireless (PBZ)"),
        [0x1E5D] = GearLink("ROG Strix Morph 96 Wireless (PBZ)"),
        [0x1D45] = GearLink("ProArt Keyboard KD300"),
        [0x1D46] = GearLink("ProArt Keyboard KD300"),
        [0x1D47] = GearLink("ProArt Keyboard KD300"),
        [0x1D68] = GearLink("ASUS TX75 Analog Gaming Keyboard"),
        [0x1D6A] = GearLink("ASUS TX75 Analog Gaming Keyboard"),
        [0x1DF3] = GearLink("ASUS TX75 Core Gaming Keyboard"),
        [0x1DF4] = GearLink("ASUS TX75 Core Gaming Keyboard"),
        [0x1DF5] = GearLink("TUF Gaming K4 Magnetic"),
        [0x1DF6] = GearLink("TUF Gaming K4 Magnetic"),
        [0x1875] = Flare("ROG Strix Flare"),
        [0x18CF] = Flare("ROG Strix Flare PNK LTD"),
        [0x18AF] = Flare("ROG Strix Flare CoD Black Ops 4 Edition"),
        [0x194B] = Flare("TUF Gaming K3"),
        [0x1B30] = Flare("TUF Gaming K3 Gen II"),
        [0x1C5E] = Flare("TUF Gaming K3 Gen II Miku Edition"),
        [0x18AA] = Flare("TUF Gaming K7"),
        [0x1899] = new("TUF Gaming K5", PerKey: false, BrightnessMax: 4, Speed: 30),
        [0x1945] = new("TUF Gaming K1", PerKey: false, BrightnessMax: 4, Speed: 1),
    };

    public static KeyboardModel? Model(ushort productId) => Models.GetValueOrDefault(productId);

    /// Keyboards with a battery: `hasPowerInfo` in their Gear Link manifest
    /// (gearlink.asus.com/view/<pid>/manifest.json, read 2026-10-09), all
    /// ids of each model. Wired-only models (Falchion Ace HFX, Strix Scope II
    /// RX, TUF K4, TX75 Core) are false there and are never asked.
    public static readonly IReadOnlySet<ushort> BatteryModels = new HashSet<ushort>
    {
        0x1A83, 0x1A85,                 // ROG Azoth
        0x1AAE, 0x1AAF, 0x1AB0,         // ROG Strix Scope II 96 Wireless
        0x1B3F, 0x1B40, 0x1B42,         // ROG Azoth Extreme
        0x1C10, 0x1C11, 0x1C12,         // ROG Azoth 96 HE / Lite
        0x1C24, 0x1C25,                 // ROG Azoth X
        0x1C2F, 0x1C31,                 // ROG Falcata
        0x1CE8, 0x1CE9, 0x1CEA,         // ROG Strix Morph 96 Wireless
        0x1CEF, 0x1CF0, 0x1CF1,         // ROG Azoth Extreme Special Edition
        0x1D45, 0x1D46, 0x1D47,         // ProArt KD300
        0x1D68, 0x1D6A,                 // ASUS TX75 Analog
        0x1DA2, 0x1DA3, 0x1DA4,         // ROG Azoth Extreme Edition 20
        0x1E0A, 0x1E0B, 0x1E0C,         // ROG Strix Morph 96 X Wireless
        0x1E5B, 0x1E5C, 0x1E5D,         // ROG Strix Morph 96 Wireless PBZ
    };

    /// The product id the receiver's paired list puts on `reportId` when it
    /// is a known keyboard, else null.
    public static ushort? PairedKeyboardProductId(IReadOnlyList<OmniMouse.Paired> paired, byte reportId) =>
        paired.Where(p => p.ReportId == reportId && Model(p.ProductId) is { Receiver: false })
            .Select(p => (ushort?)p.ProductId).FirstOrDefault();

    /// Every keyboard found: directly connected models on their vendor
    /// collection, and keyboards paired to a receiver on their channel.
    public static IReadOnlyList<AsusKeyboardDevice> Discover(IReadOnlyList<HidDeviceInfo> hid)
    {
        var found = new List<AsusKeyboardDevice>();
        var candidates = hid
            .Where(d => d.VendorId == AuraUsb.AsusVendorId && Model(d.ProductId) is not null && d.OutputReportLength > 0)
            .Where(d => Model(d.ProductId)!.Receiver
                ? d.UsagePage is >= VendorPage and <= VendorPage + 2
                : d.UsagePage == VendorPage)
            .OrderBy(d => d.ProductId)
            .ThenBy(d => d.UsagePage)
            .ToList();
        var directSeen = new HashSet<string>();
        var paired = candidates.Any(c => Model(c.ProductId)!.Receiver) ? OmniMouse.PairedDevices(hid) : [];
        foreach (var info in candidates)
        {
            var model = Model(info.ProductId)!;
            // A directly connected keyboard is one device even if it exposes
            // more than one vendor collection.
            var deviceKey = DeviceKey(info.Path);
            if (!model.Receiver && !directSeen.Add(deviceKey))
                continue;
            var reportId = info.OutputReportId ?? 0;
            IHidDevice? device = null;
            try
            {
                device = Hid.Open(info);
                var layout = Ask(device, info, reportId, GetLayout);
                if (model.Receiver && (layout is null || !IsKeyboardLayoutReply(layout, reportId))
                    && !IsPairedKeyboard(paired, reportId))
                {
                    // The mouse's or the receiver's own channel — or a keyboard
                    // that answers differently; the reply shows which.
                    LightingLog.Discovery($"[rigstats-control] Lighting: receiver channel 0x{info.ProductId:X4} report 0x{reportId:X2} " +
                        $"is not a keyboard (layout reply {Hex(layout, 8)}).");
                    device.Dispose();
                    continue;
                }
                var version = Ask(device, info, reportId, GetVersion);
                // The id stays per model ("asus-keyboard-1ace-1"); behind a
                // receiver the name is the paired keyboard's own, like the mouse's.
                var nth = found.Count(f => f._model.Name == model.Name) + 1;
                var name = model.Receiver ? PairedKeyboardName(paired, reportId) ?? model.Name : model.Name;
                var nthName = found.Count(f => f.Name == name || f.Name.StartsWith(name + " (", StringComparison.Ordinal)) + 1;
                LightingLog.Discovery($"[rigstats-control] Lighting: {model.Name} 0x{info.ProductId:X4} report 0x{reportId:X2}" +
                    (name != model.Name ? $" ({name})" : "") + $", layout {Hex(layout, 8)}, version {Hex(version, 16)}.");
                var batteryId = model.Receiver ? PairedKeyboardProductId(paired, reportId) : info.ProductId;
                found.Add(new AsusKeyboardDevice(info, device, model, reportId,
                    $"asus-keyboard-{info.ProductId:x4}-{nth}",
                    nthName == 1 ? name : $"{name} ({nthName})",
                    Hex(version, 64), Hex(layout, 64),
                    batteryId is { } id && BatteryModels.Contains(id)));
            }
            catch (Exception e)
            {
                LightingLog.Discovery($"[rigstats-control] Lighting: {model.Name} 0x{info.ProductId:X4} not usable: {e.Message}");
                device?.Dispose();
            }
        }
        return found;
    }

    /// `hid` without the collections of keyboards driven here directly —
    /// for LampArray discovery, so a keyboard isn't driven twice. Receiver
    /// collections stay: the receiver's LampArray is a paired mouse.
    public static IReadOnlyList<HidDeviceInfo> WithoutDirectKeyboards(
        IReadOnlyList<HidDeviceInfo> hid, IEnumerable<AsusKeyboardDevice> keyboards) =>
        WithoutAsusProducts(hid, keyboards.Select(k => k.DirectProductId).OfType<ushort>());

    /// `hid` without the ASUS collections of these product ids (pure, tested).
    public static IReadOnlyList<HidDeviceInfo> WithoutAsusProducts(
        IReadOnlyList<HidDeviceInfo> hid, IEnumerable<ushort> productIds)
    {
        var driven = productIds.ToHashSet();
        return hid.Where(h => !(h.VendorId == AuraUsb.AsusVendorId && driven.Contains(h.ProductId))).ToList();
    }

    /// The device part of a HID path, without the collection suffix — so
    /// two collections of one keyboard count once.
    private static string DeviceKey(string path)
    {
        var col = path.IndexOf("&col", StringComparison.OrdinalIgnoreCase);
        return col >= 0 ? path[..col] : path;
    }

    private static string Hex(byte[]? reply, int max) =>
        reply is null ? "-" : Convert.ToHexString(reply[..Math.Min(max, reply.Length)]);

    /// "Get layout" answered with a layout: the echo `12 12` then a
    /// non-zero layout. A mouse channel answers with zeros, the receiver's
    /// own channel not at all.
    public static bool IsKeyboardLayoutReply(byte[] reply, byte reportId) =>
        reply.Length >= 8 && reply[0] == reportId && reply[1] == Get && reply[2] == GetLayout
        && reply.AsSpan(3, 5).IndexOfAnyExcept((byte)0) >= 0;

    /// A paired keyboard on this receiver channel, by the receiver's own
    /// paired list — a known keyboard product id on `reportId`. Doesn't
    /// depend on how the keyboard answers "get layout", which a firmware
    /// update can change.
    public static bool IsPairedKeyboard(IReadOnlyList<OmniMouse.Paired> paired, byte reportId) =>
        PairedKeyboardName(paired, reportId) is not null;

    /// The model name of the keyboard the receiver's paired list puts on
    /// `reportId` ("ROG Azoth X"), or null when it names no known keyboard.
    public static string? PairedKeyboardName(IReadOnlyList<OmniMouse.Paired> paired, byte reportId) =>
        paired.Where(p => p.ReportId == reportId)
            .Select(p => Model(p.ProductId))
            .FirstOrDefault(m => m is { Receiver: false })?.Name;

    private static byte[]? Ask(IHidDevice device, HidDeviceInfo info, byte reportId, byte what) =>
        Ask(device, info.OutputReportLength, reportId, what, ReplyTimeout);

    /// Asks `12 <what>` and returns the reply that echoes it, skipping other
    /// input reports a woken keyboard may send first (as `OmniMouse.Ask`
    /// does); null when none came in time.
    public static byte[]? Ask(IHidDevice device, int length, byte reportId, byte what, TimeSpan timeout)
    {
        var request = new byte[length];
        request[0] = reportId;
        request[1] = Get;
        request[2] = what;
        device.Write(request);
        var deadline = DateTime.UtcNow + timeout;
        for (var left = timeout; left > TimeSpan.Zero; left = deadline - DateTime.UtcNow)
        {
            if (device.Read(left) is not { } reply)
                return null;
            if (reply.Length >= 3 && reply[0] == reportId && reply[1] == Get && reply[2] == what)
                return reply;
        }
        return null;
    }

    public void Apply(AuraEffect effect, byte red, byte green, byte blue)
    {
        lock (_lock)
            Send(EffectReport(_model, _reportId, _info.OutputReportLength, effect, red, green, blue));
    }

    /// The keyboard keeps its last effect; its own saved one returns after
    /// it sleeps or reconnects.
    public void Release() { }

    public JsonObject Diagnostics() => new()
    {
        ["vendor_id"] = $"{_info.VendorId:X4}",
        ["product_id"] = $"{_info.ProductId:X4}",
        ["interface"] = _info.Interface,
        ["usage_page"] = $"{_info.UsagePage:X4}",
        ["report_id"] = $"{_reportId:X2}",
        ["model"] = _model.Name,
        ["brightness_max"] = _model.BrightnessMax,
        ["speed"] = _model.Speed,
        ["version_reply"] = _version,
        ["layout_reply"] = _layout,
    };

    /// `[report id, 0x51, 0x2C, mode, 0, speed, brightness, colour mode,
    /// direction, (0x02,) R, G, B]` — mode 0 static, 1 breathing, 2 colour
    /// cycle; per-key keyboards carry the 0x02 marker before the colour.
    /// Off is static black. The colour arrives already scaled by the
    /// profile's brightness, so the keyboard's own brightness stays at max.
    public static byte[] EffectReport(KeyboardModel model, byte reportId, int length, AuraEffect effect,
        byte red, byte green, byte blue)
    {
        var report = new byte[Math.Max(length, 13)];
        report[0] = reportId;
        report[1] = Set;
        report[2] = SetEffect;
        report[3] = effect switch
        {
            AuraEffect.Breathing => 0x01,
            AuraEffect.SpectrumCycle => 0x02,
            _ => 0x00,
        };
        report[5] = model.Speed;
        report[6] = (byte)model.BrightnessMax;
        var colour = 9;
        if (model.PerKey)
            report[colour++] = PerKeyMarker;
        if (effect != AuraEffect.Off)
        {
            report[colour] = red;
            report[colour + 1] = green;
            report[colour + 2] = blue;
        }
        return report;
    }

    public bool HasBattery { get; }

    /// The Omni receiver is 2.4 GHz; a model's own dongle says so in its name.
    public string Connection => Lighting.Connection.Of(_info.Path, $"{_info.Product} {_model.Name}", _model.Receiver);

    /// `12 01` — the keyboards' power question (Gear Link's keyboard power
    /// classes and G-Helper agree). Only asked when the model has a battery.
    public BatteryStatus? ReadBattery()
    {
        if (!HasBattery)
            return null;
        lock (_lock)
        {
            byte[]? reply;
            try
            {
                reply = Ask(_device ?? throw new ObjectDisposedException(Name), _info.OutputReportLength, _reportId, GetPower, ReplyTimeout);
            }
            catch (Exception e) when (e is IOException or ObjectDisposedException or System.ComponentModel.Win32Exception)
            {
                _device?.Dispose();
                _device = Hid.Open(_info);
                reply = Ask(_device, _info.OutputReportLength, _reportId, GetPower, ReplyTimeout);
            }
            return reply is null ? null : BatteryReplies.Keyboard(reply);
        }
    }

    // Caller holds _lock. Reopens once (a receiver re-enumerates when a
    // paired device wakes; a USB keyboard may be replugged).
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
