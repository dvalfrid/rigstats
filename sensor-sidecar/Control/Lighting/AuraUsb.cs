using System.Text;

namespace SensorSidecar.Control.Lighting;

/// The two ASUS Aura USB controller families — same report framing,
/// different effect commands.
public enum AuraFamily
{
    /// "AURA LED Controller" on ROG/Strix/TUF/Prime boards (2019+): fixed
    /// mainboard zones plus ARGB headers; effect and colour are separate.
    Motherboard,

    /// Older addressable-only controllers: ARGB headers, effect + colour in
    /// one command.
    Addressable,
}

/// One lighting zone as the controller describes it: the mainboard's own
/// LEDs, or one ARGB header.
public sealed record AuraZone(string Id, string Name, bool Addressable, int EffectChannel, int Leds);

public enum AuraEffect
{
    Off,
    Static,
    Breathing,
    SpectrumCycle,
}

/// ASUS Aura USB HID protocol (#192). OpenRGB (GPL) was used as protocol
/// documentation only — no code copied. Every report is 65 bytes with
/// report id 0xEC; the controller describes its own zones in a config
/// table, so no board is hard-coded.
public static class AuraUsb
{
    public const ushort AsusVendorId = 0x0B05;
    public const byte ReportId = 0xEC;
    public const int ReportLength = 65;

    /// The vendor usage page of the Aura collection (a controller exposes
    /// several HID collections; this is the one that speaks Aura).
    public const ushort AuraUsagePage = 0xFF72;

    private const byte RequestFirmware = 0x82;
    private const byte RequestConfigTable = 0xB0;
    private const byte FirmwareReply = 0x02;
    private const byte ConfigTableReply = 0x30;
    private const byte MotherboardEffect = 0x35;
    private const byte MotherboardEffectColor = 0x36;
    private const byte AddressableEffect = 0x3B;

    /// Known controller product ids (OpenRGB's detector list). Also the
    /// source of the supported-devices list (<see cref="LightingCatalog"/>).
    public static readonly IReadOnlyDictionary<ushort, AuraFamily> Families = new Dictionary<ushort, AuraFamily>
    {
        [0x18F3] = AuraFamily.Motherboard,
        [0x1939] = AuraFamily.Motherboard,
        [0x19AF] = AuraFamily.Motherboard,
        [0x1AA6] = AuraFamily.Motherboard,
        [0x1BED] = AuraFamily.Motherboard,
        [0x1867] = AuraFamily.Addressable,
        [0x1872] = AuraFamily.Addressable,
        [0x18A3] = AuraFamily.Addressable,
        [0x18A5] = AuraFamily.Addressable,
    };

    /// Controllers seen working on real hardware (a PRIME B650M-A's).
    public static readonly IReadOnlySet<ushort> Verified = new HashSet<ushort> { 0x19AF };

    public static AuraFamily? Family(ushort productId) =>
        Families.TryGetValue(productId, out var family) ? family : null;

    public static byte[] FirmwareRequest() => Report(RequestFirmware);

    public static byte[] ConfigTableRequest() => Report(RequestConfigTable);

    /// "AULA3-AR32-0218"-style firmware string, or null if not a reply.
    public static string? ParseFirmware(byte[] reply)
    {
        if (reply.Length < 18 || reply[0] != ReportId || reply[1] != FirmwareReply)
            return null;
        return Encoding.ASCII.GetString(reply, 2, 16).TrimEnd('\0', ' ');
    }

    /// The 60-byte config table, or null if not a reply.
    public static byte[]? ParseConfigTable(byte[] reply)
    {
        if (reply.Length < 64 || reply[0] != ReportId || reply[1] != ConfigTableReply)
            return null;
        return reply[4..64];
    }

    /// The zones a config table describes. Motherboard: the mainboard's own
    /// LEDs (count at 0x1B) as one zone on effect channel 0, then one zone
    /// per ARGB header (count at 0x02) on the following channels.
    /// Addressable: one zone per ARGB header.
    public static IReadOnlyList<AuraZone> Zones(AuraFamily family, byte[] table)
    {
        var zones = new List<AuraZone>();
        var argbHeaders = table[0x02];
        var channel = 0;
        if (family == AuraFamily.Motherboard)
        {
            var mainboardLeds = table[0x1B];
            if (mainboardLeds > 0)
                zones.Add(new AuraZone("mainboard", "Motherboard", Addressable: false, channel++, mainboardLeds));
        }
        for (var i = 0; i < argbHeaders; i++)
            zones.Add(new AuraZone($"argb{i + 1}", $"ARGB header {i + 1}", Addressable: true, channel++, 1));
        return zones;
    }

    /// Aura effect mode numbers.
    public static byte Mode(AuraEffect effect) => effect switch
    {
        AuraEffect.Off => 0,
        AuraEffect.Static => 1,
        AuraEffect.Breathing => 2,
        AuraEffect.SpectrumCycle => 4,
        _ => 0,
    };

    /// The reports that put every zone on one effect and colour. The
    /// controller keeps it until power-off; nothing is committed to its
    /// flash (no wear, and the BIOS default comes back after a cold boot —
    /// the service re-applies the active profile anyway).
    public static IReadOnlyList<byte[]> SetEffect(AuraFamily family, IReadOnlyList<AuraZone> zones,
        AuraEffect effect, byte red, byte green, byte blue)
    {
        var mode = Mode(effect);
        if (effect == AuraEffect.Off)
            (red, green, blue) = (0, 0, 0);
        var reports = new List<byte[]>();
        if (family == AuraFamily.Addressable)
        {
            foreach (var zone in zones)
                reports.Add(Report(AddressableEffect, (byte)zone.EffectChannel, 0x00, mode, red, green, blue));
            return reports;
        }

        // Motherboard: one effect report per zone, then one colour report
        // per zone addressing its LEDs by a bit mask over all zones' LEDs.
        var start = 0;
        foreach (var zone in zones)
        {
            reports.Add(Report(MotherboardEffect, (byte)zone.EffectChannel, 0x00, 0x00, mode));
            var count = Math.Min(zone.Leds, Math.Max(0, 16 - start));
            if (count > 0)
            {
                var mask = (ushort)(((1 << count) - 1) << start);
                var color = Report(MotherboardEffectColor, (byte)(mask >> 8), (byte)(mask & 0xFF), 0x00);
                for (var led = 0; led < count; led++)
                {
                    var offset = 5 + 3 * (start + led);
                    if (offset + 2 >= ReportLength)
                        break;
                    color[offset] = red;
                    color[offset + 1] = green;
                    color[offset + 2] = blue;
                }
                reports.Add(color);
            }
            start += zone.Leds;
        }
        return reports;
    }

    /// Brightness 0–1 applied to a colour (Aura effects have no separate
    /// brightness; spectrum cycle ignores the colour entirely).
    public static (byte Red, byte Green, byte Blue) Scale(byte red, byte green, byte blue, double brightness)
    {
        var b = Math.Clamp(brightness, 0, 1);
        return ((byte)Math.Round(red * b), (byte)Math.Round(green * b), (byte)Math.Round(blue * b));
    }

    /// "#ff0033" → bytes; null when malformed.
    public static (byte Red, byte Green, byte Blue)? ParseColor(string? hex)
    {
        if (hex is not { Length: 7 } || hex[0] != '#')
            return null;
        try
        {
            return (Convert.ToByte(hex[1..3], 16), Convert.ToByte(hex[3..5], 16), Convert.ToByte(hex[5..7], 16));
        }
        catch (FormatException)
        {
            return null;
        }
    }

    private static byte[] Report(params byte[] body)
    {
        var report = new byte[ReportLength];
        report[0] = ReportId;
        body.CopyTo(report, 1);
        return report;
    }
}
