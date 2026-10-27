using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// A wired Corsair mouse or mousepad on Corsair's "V2" peripheral protocol
/// (#620): the M65 RGB Ultra (wired) and the MM700. Off or one colour on all
/// three LEDs, over the vendor collection (interface 1, usage page 0xFF42,
/// usage 1, 65-byte output reports with report id 0).
///
/// The protocol comes from OpenRGB's `CorsairPeripheralV2Controller` (GPL-2,
/// read as documentation, no code copied); the frames, their sources and
/// every open question are in `docs/corsair-v2-protocol.md`, and only what
/// its "What RIGStats will and won't send" section allows is sent:
/// - first use: the endpoint check (START `0D 00 22`, START `0D 00 01` only
///   when the device answers the first with a non-zero status, then STOP
///   `05 01 00`), SET mode = software `01 03 00 02`, GET `5F` — in OpenRGB's
///   order;
/// - every colour: START `0D 00 <endpoint>`, one block write
///   `06 00 09 00 00 00` + nine planar bytes (R0 R1 R2 G0 G1 G2 B0 B1 B2) from
///   report byte 8, STOP `05 01 00`; off is all nine bytes zero;
/// - `Release()` / service stop: SET mode = hardware `01 03 00 01`, only when
///   this instance switched the device to software mode.
///
/// Not sent: GET `12` (PID — known from USB), GET `11` (VID), GET `41`
/// (keyboard layout; neither device has a matrix), the wireless write ids,
/// `BLK_WN`, anything that saves to the device. No keep-alive: OpenRGB
/// re-sends every 30 s for the K55 RGB PRO's sake; whether these two need it
/// is unconfirmed. `Verified = false` until a user confirms.
public sealed class CorsairV2Device : ILightingDevice, IDisposable
{
    public const ushort CorsairVendorId = 0x1B1C;
    public const ushort VendorUsagePage = 0xFF42;
    public const ushort VendorUsage = 0x0001;
    public const int Interface = 1;
    public const int ReportLength = 65;

    /// Byte 1: the write id for a wired device (`CORSAIR_V2_WRITE_WIRED_ID`).
    public const byte WiredWriteId = 0x08;
    public const byte CmdSet = 0x01;
    public const byte CmdGet = 0x02;
    public const byte CmdStopTransaction = 0x05;
    public const byte CmdBlockWriteFirst = 0x06;
    public const byte CmdStartTransaction = 0x0D;

    public const byte PropertyMode = 0x03;
    public const byte ModeHardware = 0x01;
    public const byte ModeSoftware = 0x02;
    /// OpenRGB's software-controller initialisation read; what it is, and
    /// whether colours show without it, is unconfirmed — kept until a test
    /// shows it isn't needed.
    public const byte PropertyLightingInit = 0x5F;

    /// The lighting endpoints OpenRGB tries: `22` first, `01` when `22` is
    /// refused.
    public const byte EndpointDefault = 0x22;
    public const byte EndpointFallback = 0x01;

    /// Both devices have three single-LED zones, sent in OpenRGB's order.
    public const int Leds = 3;
    /// Report byte where the first block packet's data starts.
    public const int BlockDataOffset = 8;

    private static readonly TimeSpan ReplyTimeout = TimeSpan.FromMilliseconds(50);
    private static readonly TimeSpan BlockReplyTimeout = TimeSpan.FromMilliseconds(3);
    private static readonly TimeSpan DrainTimeout = TimeSpan.FromMilliseconds(3);
    private const int WritePaceMs = 2;
    /// A bound on reports drained before a transaction, so a device that
    /// keeps sending can't hold the write forever.
    private const int MaxDrained = 32;

    /// Name, `Kind`, and the zone's name; the LED order within it (index 0,
    /// 1, 2 → M65: logo, scroll wheel, indicator; MM700: left, right, logo,
    /// per OpenRGB) has not been seen on a device.
    public sealed record Model(string Name, string Kind, string Zone, bool Verified = false);

    /// Every model, by product id — only the two wired ones OpenRGB drives
    /// with its software controller and that a tester has (#598).
    public static readonly IReadOnlyDictionary<ushort, Model> Models = new Dictionary<ushort, Model>
    {
        [0x1B9E] = new("Corsair M65 RGB Ultra (wired)", "mouse", "Mouse"),
        [0x1B9B] = new("Corsair MM700 RGB", "mousepad", "Mousepad"),
    };

    /// Corsair's own software — while it runs it owns these devices (#620).
    /// "iCUE" is the app's process; not yet seen on a machine. Corsair's
    /// background services are left out on purpose: they run while the app
    /// is closed (a tester has iCUE installed but not running, #620).
    public static readonly IReadOnlyList<string> CorsairSoftware = ["iCUE"];

    public static bool IsChannel(HidDeviceInfo h) =>
        h.VendorId == CorsairVendorId && Models.ContainsKey(h.ProductId) && h.UsagePage == VendorUsagePage
        && h.Usage == VendorUsage && h.Interface == Interface && h.OutputReportLength == ReportLength;

    private readonly HidDeviceInfo _info;
    private readonly Model _model;
    private readonly IHidDevice _device;
    private readonly Func<string?> _ownSoftware;
    private readonly object _write = new();
    private byte? _endpoint;
    private bool _softwareMode;

    public CorsairV2Device(HidDeviceInfo info, Model model, IHidDevice device, Func<string?>? ownSoftware = null)
    {
        _info = info;
        _model = model;
        _device = device;
        _ownSoftware = ownSoftware ?? (() => HyperXKeyboardDevice.Running(CorsairSoftware, "Corsair iCUE"));
        Zones = [new AuraZone(model.Kind, model.Zone, Addressable: false, EffectChannel: 0, Leds: Leds)];
    }

    public string Id => $"corsair-{_info.VendorId:x4}-{_info.ProductId:x4}";
    public string Name => _model.Name;
    public string Kind => _model.Kind;
    public string Firmware => "";
    public IReadOnlyList<AuraZone> Zones { get; }
    public string? Blocked => _ownSoftware() is { } program ? $"{program} is running" : null;
    public string? Problem { get; private set; }
    public bool StaticOnly => true;

    /// Opens the collection only — nothing is sent until a profile asks.
    public static IReadOnlyList<CorsairV2Device> Discover(IReadOnlyList<HidDeviceInfo> hid,
        Func<HidDeviceInfo, IHidDevice>? open = null)
    {
        open ??= Hid.Open;
        var found = new List<CorsairV2Device>();
        foreach (var info in hid.Where(IsChannel).OrderBy(h => h.Path, StringComparer.OrdinalIgnoreCase))
        {
            var model = Models[info.ProductId];
            try
            {
                found.Add(new CorsairV2Device(info, model, open(info)));
                LightingLog.Discovery($"[rigstats-control] Lighting: {model.Name} {info.VendorId:X4}:{info.ProductId:X4} (Corsair V2 protocol).");
            }
            catch (Exception e) when (e is IOException or System.ComponentModel.Win32Exception or UnauthorizedAccessException)
            {
                LightingLog.Discovery($"[rigstats-control] Lighting: {model.Name} couldn't be opened: {e.Message}");
            }
        }
        return found;
    }

    /// Never throws (not confirmed yet): a failure shows as `Problem`.
    /// Breathing and spectrum show as the colour; off is black.
    public void Apply(AuraEffect effect, byte red, byte green, byte blue)
    {
        try
        {
            var (r, g, b) = effect == AuraEffect.Off ? ((byte)0, (byte)0, (byte)0) : (red, green, blue);
            lock (_write)
            {
                if (!_softwareMode)
                    TakeControl();
                WriteColour(r, g, b);
            }
            Problem = null;
        }
        catch (Exception e) when (e is IOException or System.ComponentModel.Win32Exception or InvalidOperationException or OperationCanceledException)
        {
            Problem = e.Message;
            SidecarLog.Log($"[rigstats-control] Lighting on {Name} failed: {e.Message}");
        }
    }

    /// Back to the device's own lighting: SET mode = hardware, as OpenRGB
    /// does on close (a device left in software mode stays there until it
    /// is plugged in again). Only when this instance took control — a device
    /// RIGStats never wrote (or one iCUE owns) is left alone. Best-effort.
    public bool Release()
    {
        lock (_write)
        {
            if (!_softwareMode)
                return false;
            try
            {
                Send(SetModeReport(ModeHardware), ReplyTimeout);
                _softwareMode = false;
                return true;
            }
            catch (Exception e) when (e is IOException or System.ComponentModel.Win32Exception or InvalidOperationException or OperationCanceledException)
            {
                SidecarLog.Log($"[rigstats-control] Lighting release on {Name} failed: {e.Message}");
                return false;
            }
        }
    }

    public JsonObject Diagnostics() => new()
    {
        ["vendor_id"] = $"{_info.VendorId:X4}",
        ["product_id"] = $"{_info.ProductId:X4}",
        ["interface"] = _info.Interface,
        ["product"] = _info.Product,
        ["protocol"] = "corsair_v2",
        ["endpoint"] = _endpoint is { } e ? $"{e:X2}" : null,
        ["verified"] = _model.Verified,
    };

    public void Dispose() => _device.Dispose();

    // The endpoint check, then software mode and OpenRGB's initialisation
    // read — once per instance (the endpoint is kept after a release). The
    // replies are logged once: they answer the doc's open questions (which
    // endpoint, whether bytes 1–2 echo the frame).
    private void TakeControl()
    {
        if (_endpoint is null)
        {
            // Endpoint, decided the way OpenRGB does (the doc's "will send"):
            // START on 22; only a reply with a non-zero status (report byte 3)
            // moves to 01. No reply at all leaves 22 — OpenRGB's read buffer
            // stays zero then, which it reads as status 00.
            Drain();
            var first = Send(StartTransactionReport(EndpointDefault), ReplyTimeout);
            var endpoint = EndpointDefault;
            byte[]? second = null;
            if (Status(first) is not (null or 0))
            {
                endpoint = EndpointFallback;
                second = Send(StartTransactionReport(EndpointFallback), ReplyTimeout);
            }
            Send(StopTransactionReport(), ReplyTimeout);
            _endpoint = endpoint;
            SidecarLog.Log($"[rigstats-control] Lighting: {Name} endpoint {endpoint:X2} (START 22 → {ReplyText(first)}"
                + (second is null ? "" : $", START 01 → {ReplyText(second)}") + ").");
        }
        var mode = Send(SetModeReport(ModeSoftware), ReplyTimeout);
        _softwareMode = true;
        var init = Send(GetReport(PropertyLightingInit), ReplyTimeout);
        SidecarLog.Log($"[rigstats-control] Lighting: {Name} software mode → {ReplyText(mode)}, GET 5F → {ReplyText(init)}.");
    }

    private void WriteColour(byte red, byte green, byte blue)
    {
        Drain();
        Send(StartTransactionReport(_endpoint!.Value), ReplyTimeout);
        if (Send(ColourBlockReport(red, green, blue), BlockReplyTimeout) is null)
            Thread.Sleep(WritePaceMs);
        Send(StopTransactionReport(), ReplyTimeout);
    }

    /// Writes one report and reads one reply, as OpenRGB does after each
    /// write (it paces the device); null when none came in time.
    private byte[]? Send(byte[] report, TimeSpan replyTimeout)
    {
        _device.Write(report);
        return _device.Read(replyTimeout);
    }

    /// Reads (never writes) whatever input reports are pending, so the next
    /// reply read belongs to the next frame — OpenRGB's `ClearPacketBuffer`.
    private void Drain()
    {
        for (var i = 0; i < MaxDrained && _device.Read(DrainTimeout) is not null; i++)
        {
        }
    }

    private static string ReplyText(byte[]? reply) => reply is null ? "no reply" : Hid.ReplyHex(reply);

    // ── Pure (unit-tested) ────────────────────────────────────────────

    /// The status of a reply (report byte 3: `00` OK, `01` invalid value,
    /// `03` failed, `05` unsupported), or null when there was no reply.
    public static byte? Status(byte[]? reply) => reply is { Length: > 3 } ? reply[3] : null;

    /// One 65-byte report: `00 08 <command> <args…>`, zero-filled.
    public static byte[] Report(byte command, params byte[] args)
    {
        var report = new byte[ReportLength];
        report[1] = WiredWriteId;
        report[2] = command;
        args.CopyTo(report, 3);
        return report;
    }

    /// SET property 03 (mode): `01 03 00 <mode>`.
    public static byte[] SetModeReport(byte mode) => Report(CmdSet, PropertyMode, 0x00, mode);

    /// GET a property: `02 <property>`.
    public static byte[] GetReport(byte property) => Report(CmdGet, property);

    /// Start transaction on a lighting endpoint: `0D 00 <endpoint>`.
    public static byte[] StartTransactionReport(byte endpoint) => Report(CmdStartTransaction, 0x00, endpoint);

    /// Stop transaction: `05 01 00`.
    public static byte[] StopTransactionReport() => Report(CmdStopTransaction, 0x01, 0x00);

    /// The nine colour bytes, planar: three reds, three greens, three blues
    /// (OpenRGB's software controller), every LED the same colour.
    public static byte[] ColourData(byte red, byte green, byte blue)
    {
        var data = new byte[Leds * 3];
        for (var led = 0; led < Leds; led++)
        {
            data[led] = red;
            data[Leds + led] = green;
            data[2 * Leds + led] = blue;
        }
        return data;
    }

    /// The first (and only) block-write packet: `06 00 <len lo> <len hi> 00 00`
    /// and the data from report byte 8.
    public static byte[] ColourBlockReport(byte red, byte green, byte blue)
    {
        var data = ColourData(red, green, blue);
        var report = Report(CmdBlockWriteFirst, 0x00, (byte)data.Length, (byte)(data.Length >> 8));
        data.CopyTo(report, BlockDataOffset);
        return report;
    }
}
