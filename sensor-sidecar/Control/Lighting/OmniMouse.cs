using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// The mouse paired to a ROG Omni receiver (`1ACE`), reached on the
/// receiver's vendor channels (#236). Two things the mouse's LampArray
/// can't say: which mouse it is (the receiver names only itself), and its
/// "WDL state" — Gear Link's Cross-device Lighting Toggle. With WDL off
/// ("Device Lighting") the mouse accepts and ignores every LampArray write.
///
/// Protocol from Gear Link's own HID log, verified on the ROG Harpe Ace Aim
/// Lab Edition (docs/control-architecture.md):
/// receiver channel (report id 1): `a0 00` → paired devices;
/// mouse channel (the report id the receiver names): `12 00 09 00` → WDL
/// state, `51 42 00 00 0x` → set it. Every reply echoes the command.
public interface IWdlDevice
{
    /// Whether this device offers the WDL switch.
    bool HasWdl { get; }

    /// Whether the model is known, so a false `HasWdl` means "has no such
    /// setting" rather than "unknown".
    bool KnownModel { get; }

    /// On, off, or null when the device doesn't answer.
    bool? WdlOn();

    /// Lets lighting commands through ("Aura Sync & Windows Dynamic Lighting").
    void EnableWdl();
}

/// The mouse's channel is opened per question, not kept open: Windows
/// queues every input report for every open handle, so a long-lived handle
/// would first read replies to Gear Link's questions (an old WDL state).
public sealed class OmniMouse
{
    public const ushort ReceiverProductId = 0x1ACE;
    private const ushort VendorPage = 0xFF00;
    private const byte ReceiverReportId = 1;
    private static readonly TimeSpan ReplyTimeout = TimeSpan.FromMilliseconds(500);

    /// Mice by the product id the receiver reports (the mouse's 2.4 GHz
    /// id). From Gear Link's device manifests and its Companion's model
    /// configs, which agree: `Wdl` = the mouse has the Cross-device Lighting
    /// Toggle (Companion `WDL=1`, Gear Link's WDL switch) — newer mice follow
    /// LampArray's own autonomous mode and have none. `Verified`: the switch
    /// was checked on this mouse.
    public static readonly IReadOnlyDictionary<ushort, MouseModel> Models = new Dictionary<ushort, MouseModel>
    {
        [0x1A94] = new("ROG Harpe Ace Aim Lab Edition", Wdl: true, Verified: true),
        [0x1B18] = new("ROG Keris II Ace", Wdl: true),
        [0x1B65] = new("ROG Harpe Ace Mini", Wdl: true),
        [0x1B69] = new("ROG Harpe Ace Extreme", Wdl: true),
        [0x1C0E] = new("ROG Keris II Origin"),
        [0x1C6B] = new("ROG Harpe II Ace"),
        [0x1CD3] = new("ProArt Mouse MD301"),
        [0x1D4E] = new("ROG Keris II Origin (KJP)"),
        [0x1D7C] = new("ROG Harpe II Extreme Edition 20"),
        [0x1DBF] = new("ROG Gladius IV Ace"),
        [0x1DE0] = new("ROG Spatha X 65K"),
        [0x1E32] = new("ROG Harpe II Ace Mini"),
        [0x1E4B] = new("ROG Harpe II Ace (PBZ)"),
        [0x1E4F] = new("ROG Gladius IV Ace Max"),
        [0x1E52] = new("ROG Harpe II Ace Mini Demon1 Edition"),
    };

    public sealed record MouseModel(string Name, bool Wdl = false, bool Verified = false);

    private readonly object _lock = new();
    private readonly HidDeviceInfo _channel;
    private readonly byte _reportId;

    public ushort ProductId { get; }

    /// The model name, or null for a mouse not in <see cref="Models"/>.
    public string? Name => Models.TryGetValue(ProductId, out var m) ? m.Name : null;

    /// Whether this mouse has the WDL switch, so it is read and offered.
    public bool HasWdl => Models.TryGetValue(ProductId, out var m) && m.Wdl;

    private OmniMouse(HidDeviceInfo channel, byte reportId, ushort productId)
    {
        _channel = channel;
        _reportId = reportId;
        ProductId = productId;
    }

    /// One paired device: its product id and the report id of its channel.
    public readonly record struct Paired(ushort ProductId, byte ReportId);

    /// The paired list from an `a0 00` reply (report id first):
    /// `01 a0 00 <count>`, then per device `<slot> <pid lo> <pid hi>
    /// <report id> <type>`. Seen with one device; entries that don't look
    /// like a device (pid 0, report id outside 1–3) are skipped.
    public static IReadOnlyList<Paired> ParsePaired(byte[] reply)
    {
        if (reply.Length < 4 || reply[0] != ReceiverReportId || reply[1] != 0xA0 || reply[2] != 0x00)
            return [];
        var found = new List<Paired>();
        for (int i = 0, at = 4; i < reply[3] && at + 4 < reply.Length; i++, at += 5)
        {
            var pid = (ushort)(reply[at + 1] | reply[at + 2] << 8);
            var reportId = reply[at + 3];
            if (pid != 0 && reportId is >= 1 and <= 3)
                found.Add(new Paired(pid, reportId));
        }
        return found;
    }

    /// The WDL state from a `12 00 09 00` reply, or null when it isn't one.
    public static bool? ParseWdl(byte[] reply, byte reportId) =>
        reply.Length >= 6 && reply[0] == reportId && reply[1] == 0x12 && reply[2] == 0x00 && reply[3] == 0x09
            ? reply[5] != 0
            : null;

    /// The mouse on the receiver among `hid`, or null: no receiver, more
    /// than one (which channel belongs to which is unknown), or no mouse.
    public static OmniMouse? Find(IReadOnlyList<HidDeviceInfo> hid)
    {
        var channels = hid
            .Where(h => h.VendorId == AuraUsb.AsusVendorId && h.ProductId == ReceiverProductId
                && h.UsagePage is >= VendorPage and <= VendorPage + 2 && h.OutputReportLength > 0)
            .ToList();
        var receiver = channels.Where(h => h.OutputReportId == ReceiverReportId).ToList();
        if (receiver.Count != 1)
            return null;

        IReadOnlyList<Paired> paired;
        using (var device = Hid.Open(receiver[0]))
            paired = Ask(device, [ReceiverReportId, 0xA0, 0x00]) is { } reply ? ParsePaired(reply) : [];

        foreach (var p in paired)
        {
            if (p.ReportId == ReceiverReportId || channels.FirstOrDefault(c => c.OutputReportId == p.ReportId) is not { } channel)
                continue;
            var mouse = new OmniMouse(channel, p.ReportId, p.ProductId);
            // A known mouse is taken as it is; an unknown device counts as the
            // mouse only if it answers the WDL query (a keyboard doesn't).
            var wdl = mouse.HasWdl || mouse.Name is null ? mouse.WdlOn() : null;
            if (mouse.Name is not null || wdl is not null)
            {
                LightingLog.Discovery($"[rigstats-control] Lighting: Omni receiver mouse 0x{p.ProductId:X4} " +
                    $"({mouse.Name ?? "unknown model"}) on report 0x{p.ReportId:X2}" +
                    (wdl is { } on ? $", WDL {(on ? "on" : "off")}." : "."));
                return mouse;
            }
        }
        return null;
    }

    /// The mouse's WDL state, or null when it doesn't answer (asleep, gone).
    public bool? WdlOn()
    {
        try
        {
            lock (_lock)
            {
                using var device = Hid.Open(_channel);
                return Ask(device, [_reportId, 0x12, 0x00, 0x09, 0x00]) is { } reply ? ParseWdl(reply, _reportId) : null;
            }
        }
        catch (Exception)
        {
            return null;
        }
    }

    /// Switches WDL on or off; throws when the mouse doesn't confirm it.
    public void SetWdl(bool on)
    {
        lock (_lock)
        {
            using var device = Hid.Open(_channel);
            if (Ask(device, [_reportId, 0x51, 0x42, 0x00, 0x00, on ? (byte)1 : (byte)0]) is null)
                throw new InvalidOperationException("The mouse didn't confirm the lighting mode change. Move it to wake it up and try again.");
        }
    }

    public JsonObject Diagnostics() => new()
    {
        ["paired_product_id"] = $"{ProductId:X4}",
        ["model"] = Name,
        ["report_id"] = $"{_reportId:X2}",
        // Read for any mouse here: an unknown one answering it is how a new
        // WDL model shows up in a diagnostics report.
        ["wdl_on"] = WdlOn(),
    };

    /// Writes `request` and returns the reply that echoes it (report id and
    /// up to three command bytes — `12 00 02` and `12 00 09` differ only in
    /// the third), skipping other input reports; null on timeout.
    public static byte[]? Ask(IHidDevice device, byte[] request)
    {
        var echo = Math.Min(request.Length, 4);
        device.Write(request);
        var deadline = DateTime.UtcNow + ReplyTimeout;
        for (var left = ReplyTimeout; left > TimeSpan.Zero; left = deadline - DateTime.UtcNow)
        {
            if (device.Read(left) is not { } reply)
                return null;
            if (reply.Length >= echo && reply.AsSpan(0, echo).SequenceEqual(request.AsSpan(0, echo)))
                return reply;
        }
        return null;
    }
}
