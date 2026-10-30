using System.Text;
using System.Text.RegularExpressions;
using SensorSidecar.Control.Lighting;

namespace SensorSidecar.Control;

/// Logitech devices behind a Bolt or Unifying USB receiver (#407): which
/// slots hold a device, its name and kind, and its battery. Read-only and
/// limited to documented requests (HID++ 1.0 receiver registers and the
/// HID++ 2.0 battery reads of `LogitechBattery`, as implemented by Solaar's
/// `receiver.py` / `base.py`):
///
/// - the receiver's connection count: read register `0x02` (`10 FF 81 02`);
/// - per slot N, receiver info `0xB5` (long register read `10 FF 83 B5 sub`):
///   pairing information (Bolt `0x5N`, Unifying `0x2(N-1)`) and name (Bolt
///   `0x6N 01`, Unifying `0x4(N-1)`);
/// - the device's battery with `LogitechBattery.Read`, addressed to slot N.
///
/// Windows splits the receiver's HID++ interface into a short-report
/// collection (usage `FF00:0001`, 7 bytes) and a long one (`FF00:0002`, 20
/// bytes); a request goes to the one matching its length, and a reply can come
/// on either (a register's long answer, a short error).
public static class LogitechReceiver
{
    public enum Family { Bolt, Unifying }

    public const ushort UsagePage = 0xFF00;
    private const ushort ShortUsage = 0x0001;
    private const ushort LongUsage = 0x0002;

    private const byte ShortReport = 0x10;
    private const byte LongReport = 0x11;
    private const byte Receiver = 0xFF;
    private const byte ReadRegister = 0x81;
    private const byte ReadLongRegister = 0x83;
    private const byte ErrorReply = 0x8F;
    public const byte Connections = 0x02;
    private const byte Info = 0xB5;

    /// Bolt: 6 slots (Solaar's `_bolt_receiver`); Unifying: 6.
    public const int Slots = 6;

    private static readonly TimeSpan ReplyTimeout = TimeSpan.FromSeconds(1);

    /// Solaar's `base_usb` receiver table: Bolt `C548`, Unifying `C52B` and
    /// `C532`. Other receivers (Nano, Lightspeed) aren't read.
    private static readonly Dictionary<ushort, Family> Families = new()
    {
        [0xC548] = Family.Bolt,
        [0xC52B] = Family.Unifying,
        [0xC532] = Family.Unifying,
    };

    public static IEnumerable<ushort> ProductIds => Families.Keys;

    public static Family? FamilyOf(ushort productId) => Families.TryGetValue(productId, out var family) ? family : null;

    /// Receivers seen working on real hardware, and with what — they drive
    /// docs/supported-devices.md (`BatteryCatalog`).
    public static readonly IReadOnlyDictionary<ushort, string> VerifiedWith = new Dictionary<ushort, string>
    {
        [0xC548] = "Logi M650",
    };

    public static IReadOnlySet<ushort> Verified { get; } = VerifiedWith.Keys.ToHashSet();

    /// A receiver's two HID++ collections, paired by their Windows instance
    /// (`…&mi_02&col01#b&6fd059d&0&0000#…` and `…&col02#b&6fd059d&0&0001#…`).
    public sealed record Found(string Key, Family Family, HidDeviceInfo ShortReports, HidDeviceInfo LongReports);

    public static IEnumerable<Found> Receivers(IEnumerable<HidDeviceInfo> hid)
    {
        var collections = hid.Where(h => h.VendorId == LogitechBattery.Vendor && h.UsagePage == UsagePage && FamilyOf(h.ProductId) is not null).ToList();
        foreach (var group in collections.GroupBy(h => InstanceKey(h.Path)))
        {
            if (group.Key is null)
                continue;
            if (group.FirstOrDefault(h => h.Usage == ShortUsage) is { } shortCollection
                && group.FirstOrDefault(h => h.Usage == LongUsage) is { } longCollection)
                yield return new Found(group.Key, FamilyOf(shortCollection.ProductId)!.Value, shortCollection, longCollection);
        }
    }

    /// `046d-c548-b&6fd059d&0` from a collection's path, or null.
    public static string? InstanceKey(string path) =>
        Regex.Match(path, @"vid_([0-9a-f]{4})&pid_([0-9a-f]{4})[^#]*#([^#]+)&[0-9a-f]{4}#", RegexOptions.IgnoreCase) is { Success: true } m
            ? $"{m.Groups[1].Value}-{m.Groups[2].Value}-{m.Groups[3].Value}".ToLowerInvariant()
            : null;

    /// Read register request (`10 FF 81 reg 00 00 00`).
    public static byte[] RegisterRequest(byte register) => [ShortReport, Receiver, ReadRegister, register, 0, 0, 0];

    /// Long register read of receiver info (`10 FF 83 B5 sub arg 00`).
    public static byte[] InfoRequest(byte sub, byte arg = 0) => [ShortReport, Receiver, ReadLongRegister, Info, sub, arg, 0];

    public static byte PairingSub(Family family, int slot) => (byte)(family == Family.Bolt ? 0x50 + slot : 0x20 + slot - 1);

    public static (byte Sub, byte Arg) NameSub(Family family, int slot) =>
        family == Family.Bolt ? ((byte)(0x60 + slot), (byte)0x01) : ((byte)(0x40 + slot - 1), (byte)0x00);

    /// The parameters after `81 reg` / `83 B5` when `reply` answers the
    /// request, null for anything else (a notification, another program's
    /// reply). A receiver error (`8F`) for it throws.
    public static byte[]? Answer(byte[] reply, byte[] request)
    {
        if (reply.Length < 6 || reply[1] != Receiver)
            return null;
        if (reply[0] == ShortReport && reply[2] == ErrorReply && reply[3] == request[2] && reply[4] == request[3])
            throw new InvalidOperationException($"HID++ error {reply[5]:X2}");
        if (reply[2] != request[2] || reply[3] != request[3])
            return null;
        // A receiver-info answer also echoes its sub-register.
        if (request[2] == ReadLongRegister && reply[4] != request[4])
            return null;
        return reply[4..];
    }

    /// Connected devices, from register `0x02`'s second parameter byte.
    public static int ConnectionCount(byte[] answer) => answer.Length > 1 ? answer[1] : 0;

    /// The kind code in a pairing-information answer (Bolt byte 1, Unifying
    /// byte 7, low nibble).
    public static string Kind(Family family, byte[] pairing)
    {
        var at = family == Family.Bolt ? 1 : 7;
        return pairing.Length > at ? KindName(pairing[at] & 0x0F) : "device";
    }

    /// Solaar's `DEVICE_KIND`: 1 keyboard, 2 mouse, 8 trackball, 9 touchpad,
    /// 13 headset.
    public static string KindName(int code) => code switch
    {
        0x01 => "keyboard",
        0x02 or 0x08 or 0x09 => "mouse",
        0x0D => "headset",
        _ => "device",
    };

    /// The name in a name answer: Bolt `sub 01 length name…`, Unifying
    /// `sub length name…`.
    public static string? Name(Family family, byte[] answer)
    {
        var at = family == Family.Bolt ? 2 : 1;
        if (answer.Length <= at)
            return null;
        var length = Math.Min(answer[at], answer.Length - at - 1);
        var name = Encoding.ASCII.GetString(answer, at + 1, length).TrimEnd('\0', ' ');
        return name.Length > 0 ? name : null;
    }

    /// One register read, the answer's parameters or null on no answer.
    public static byte[]? Ask(IHidDevice receiver, byte[] request)
    {
        receiver.Write(request);
        var deadline = DateTime.UtcNow + ReplyTimeout;
        while (DateTime.UtcNow < deadline)
        {
            if (receiver.Read(deadline - DateTime.UtcNow) is not { } reply)
                return null;
            if (Answer(reply, request) is { } answer)
                return answer;
        }
        return null;
    }

    /// The paired devices: slot, name, kind. A slot whose pairing register
    /// errors is empty.
    public static IReadOnlyList<(int Slot, string Name, string Kind)> Paired(IHidDevice receiver, Family family)
    {
        var paired = new List<(int, string, string)>();
        for (var slot = 1; slot <= Slots; slot++)
        {
            byte[]? pairing;
            try
            {
                pairing = Ask(receiver, InfoRequest(PairingSub(family, slot)));
            }
            catch (InvalidOperationException)
            {
                continue; // nothing paired here
            }
            if (pairing is null)
                continue;
            var (sub, arg) = NameSub(family, slot);
            string? name = null;
            try
            {
                if (Ask(receiver, InfoRequest(sub, arg)) is { } answer)
                    name = Name(family, answer);
            }
            catch (InvalidOperationException)
            {
                // No name: the slot still counts.
            }
            paired.Add((slot, name ?? $"Logitech device {slot}", Kind(family, pairing)));
        }
        return paired;
    }
}

/// A receiver's short and long HID++ collections as one device: a request
/// goes to the collection of its length, replies are read from both.
public sealed class ReceiverHid(IHidDevice shortCollection, IHidDevice longCollection) : IHidDevice
{
    private static readonly TimeSpan Slice = TimeSpan.FromMilliseconds(20);

    public void Write(byte[] report) => (report.Length > 7 ? longCollection : shortCollection).Write(report);

    public byte[]? Read(TimeSpan timeout)
    {
        var deadline = DateTime.UtcNow + timeout;
        while (true)
        {
            if (longCollection.Read(Slice) is { } longReply)
                return longReply;
            if (shortCollection.Read(Slice) is { } shortReply)
                return shortReply;
            if (DateTime.UtcNow >= deadline)
                return null;
        }
    }

    public void Dispose()
    {
        shortCollection.Dispose();
        longCollection.Dispose();
    }
}

/// One device behind a receiver, as the battery monitor sees it.
public sealed class LogitechReceiverDevice(LogitechReceiver.Found receiver, int slot, string name, string kind) : IBatteryDevice
{
    private (ushort Feature, byte Index)? _feature;

    public string Id => $"logi:{receiver.Key}:{slot}";
    public string Name => name;
    public string Kind => kind;
    public bool HasBattery => true;
    public string Connection => Lighting.Connection.Radio;

    /// Null when the device doesn't answer in time; a receiver error (the
    /// device is asleep or off) throws, which the monitor logs once.
    public BatteryStatus? ReadBattery()
    {
        using var hid = new ReceiverHid(Hid.Open(receiver.ShortReports), Hid.Open(receiver.LongReports));
        var (reading, found) = LogitechBattery.Read(hid, _feature, (byte)slot);
        _feature = found ?? _feature;
        return reading is { Percent: { } percent } r ? new BatteryStatus(percent, r.Charging) : null;
    }
}

/// The devices behind every Bolt or Unifying receiver; the paired list is
/// read again only when the receiver's connection count changes or a new
/// receiver appears. A receiver that fails is logged once.
public sealed class LogitechReceivers
{
    private readonly Dictionary<string, (int Count, IReadOnlyList<LogitechReceiverDevice> Devices)> _known = [];
    private readonly HashSet<string> _failureLogged = [];

    public IReadOnlyList<IBatteryDevice> Devices()
    {
        var result = new List<IBatteryDevice>();
        var present = new HashSet<string>();
        foreach (var receiver in LogitechReceiver.Receivers(Hid.Enumerate()))
        {
            present.Add(receiver.Key);
            try
            {
                using var hid = new ReceiverHid(Hid.Open(receiver.ShortReports), Hid.Open(receiver.LongReports));
                var count = LogitechReceiver.Ask(hid, LogitechReceiver.RegisterRequest(LogitechReceiver.Connections)) is { } answer
                    ? LogitechReceiver.ConnectionCount(answer)
                    : -1;
                if (!_known.TryGetValue(receiver.Key, out var known) || known.Count != count)
                {
                    var paired = LogitechReceiver.Paired(hid, receiver.Family);
                    known = (count, [.. paired.Select(p => new LogitechReceiverDevice(receiver, p.Slot, p.Name, p.Kind))]);
                    _known[receiver.Key] = known;
                    SidecarLog.Log($"[rigstats-control] Battery: Logitech {receiver.Family} receiver {receiver.ShortReports.ProductId:X4}: "
                        + (paired.Count == 0 ? "nothing paired" : string.Join(", ", paired.Select(p => $"slot {p.Slot} {p.Name} ({p.Kind})"))) + ".");
                }
                result.AddRange(known.Devices);
                _failureLogged.Remove(receiver.Key);
            }
            catch (Exception e)
            {
                if (_failureLogged.Add(receiver.Key))
                    SidecarLog.Log($"[rigstats-control] Battery: Logitech receiver {receiver.ShortReports.ProductId:X4} not read: {e.Message}");
            }
        }
        foreach (var gone in _known.Keys.Where(k => !present.Contains(k)).ToList())
            _known.Remove(gone);
        return result;
    }
}
