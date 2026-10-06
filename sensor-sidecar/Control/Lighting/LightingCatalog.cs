using System.Net;
using System.Text;

namespace SensorSidecar.Control.Lighting;

/// One row of the supported-devices list.
public sealed record SupportedDevice(string Name, string Type, string UsbIds, string Status, string Notes);

/// The supported lighting devices, built from the drivers' own model tables
/// — so the list in docs/supported-devices.md and on the website can't
/// drift from the code. A test compares both files with what this produces
/// (and rewrites them when RIGSTATS_UPDATE_SUPPORTED_DEVICES=1).
public static class LightingCatalog
{
    public const string Verified = "Verified on hardware";
    public const string FromOpenRgb = "From OpenRGB, not yet verified";

    public static IReadOnlyList<SupportedDevice> All()
    {
        var rows = new List<SupportedDevice>
        {
            Family("ASUS Aura USB motherboard controller (ROG, Strix, TUF, Prime boards, 2019+)", AuraFamily.Motherboard,
                "Onboard LEDs and every ARGB header, as the controller describes them"),
            Family("ASUS Aura USB addressable controller (older boards)", AuraFamily.Addressable,
                "ARGB headers"),
        };

        rows.AddRange(AsusMonitorDevice.Models
            .OrderBy(m => m.Value.Kind == "light_bar").ThenByDescending(m => m.Value.Verified).ThenBy(m => m.Value.Name)
            .Select(m => new SupportedDevice(
                m.Value.Name,
                m.Value.Kind == "light_bar" ? "Light bar" : "Monitor",
                Hex(m.Key),
                m.Value.Verified ? Verified : FromOpenRgb,
                string.Join("; ", new[]
                {
                    m.Value.BuiltInEffects ? "Runs its own effects" : "Effects drawn by RIGStats",
                    m.Value.Kind == "light_bar" ? "Desk lamp: on/off, brightness, colour temperature" : null,
                }.OfType<string>()))));

        rows.AddRange(AsusKeyboardDevice.Models
            .OrderByDescending(m => m.Value.Verified).ThenBy(m => m.Value.Name)
            .Select(m => new SupportedDevice(
                m.Value.Receiver ? "Keyboards paired to the ROG Omni receiver (e.g. ROG Azoth X)" : m.Value.Name,
                "Keyboard",
                Hex(m.Key),
                m.Value.Verified ? Verified : FromOpenRgb,
                m.Value.Receiver ? "Found on whichever receiver channel the keyboard answers" : "Runs its own effects")));

        rows.AddRange(AsusHeadsetDevice.Models
            .OrderBy(m => m.Value)
            .Select(m => new SupportedDevice(m.Value, "Headset", Hex(m.Key), Verified,
                "Through its 2.4 GHz dongle; found when switched on")));

        rows.Add(new SupportedDevice(
            "Any Windows Dynamic Lighting (HID LampArray) device, any brand",
            "Keyboard, mouse, other",
            "—",
            Verified + " (ROG Harpe Ace on the Omni receiver)",
            "Skipped while Windows Dynamic Lighting controls it; keyboards above use their own protocol instead. " +
            "ASUS devices: set the device's Cross-device Lighting Toggle to \"Aura Sync & Windows Dynamic Lighting\" " +
            "in Gear Link or Armoury Crate, otherwise it ignores every lighting change"));

        rows.Add(new SupportedDevice(
            "Philips Hue lights, through a Hue Bridge (square, v2)",
            "Room lights",
            "— (network)",
            Verified,
            "Paired from the Control Center; only the rooms and zones you choose follow the rig. " +
            "Breathing and spectrum cycle fade slowly (the bridge takes about one command a second)"));
        return rows;
    }

    /// docs/supported-devices.md.
    public static string Markdown()
    {
        var md = new StringBuilder();
        md.Append("# Supported lighting devices\n\n");
        md.Append("<!-- Generated from the sensor sidecar's model tables by LightingCatalog — do not edit by hand.\n");
        md.Append("     Regenerate: $env:RIGSTATS_UPDATE_SUPPORTED_DEVICES=1; dotnet test sensor-sidecar.Tests --filter SupportedDevices -->\n\n");
        md.Append("RIGStats drives these devices natively — no Armoury Crate, OpenRGB or other software needed. ");
        md.Append($"**{Verified}**: seen working on real hardware. **{FromOpenRgb}**: same protocol as a verified device, ");
        md.Append("listed from OpenRGB's device list (read as documentation) — it should work; ");
        md.Append("[open an issue](https://github.com/dvalfrid/rigstats/issues) with your diagnostics ZIP if it doesn't, or to confirm it does.\n\n");
        md.Append("| Device | Type | USB id | Status | Notes |\n|---|---|---|---|---|\n");
        foreach (var row in All())
            md.Append($"| {row.Name} | {row.Type} | {row.UsbIds} | {row.Status} | {row.Notes} |\n");
        md.Append("\nNot supported: RGB on memory modules and graphics cards (reached over SMBus/I²C, where a wrong ");
        md.Append("write can damage the hardware). A device that isn't listed: the diagnostics export's ");
        md.Append("`lighting-devices.json` lists every HID device on the machine — attach it to an issue.\n");
        return md.ToString();
    }

    /// The website's table body, between the supported-devices markers.
    public static string HtmlRows()
    {
        var html = new StringBuilder();
        foreach (var row in All())
        {
            var status = row.Status.StartsWith(Verified, StringComparison.Ordinal) ? "status-verified" : "status-openrgb";
            html.Append("                <tr>")
                .Append($"<td>{Enc(row.Name)}</td>")
                .Append($"<td>{Enc(row.Type)}</td>")
                .Append($"<td class=\"{status}\">{Enc(row.Status)}</td>")
                .Append($"<td>{Enc(row.Notes)}</td>")
                .Append("</tr>\n");
        }
        return html.ToString();
    }

    private static SupportedDevice Family(string name, AuraFamily family, string notes)
    {
        var ids = AuraUsb.Families.Where(f => f.Value == family).Select(f => f.Key).Order().ToList();
        var verified = ids.Where(AuraUsb.Verified.Contains).ToList();
        var status = verified.Count == ids.Count ? Verified
            : verified.Count > 0 ? $"{Verified} ({string.Join(", ", verified.Select(Hex))}); the others from OpenRGB"
            : FromOpenRgb;
        return new SupportedDevice(name, "Motherboard", string.Join(", ", ids.Select(Hex)), status, notes);
    }

    private static string Hex(ushort id) => $"{id:X4}";

    private static string Enc(string text) => WebUtility.HtmlEncode(text);
}
