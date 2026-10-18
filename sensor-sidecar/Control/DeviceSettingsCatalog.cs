using System.Globalization;
using System.Text;
using SensorSidecar.Control.Lighting;
using SensorSidecar.Control.Mice;

namespace SensorSidecar.Control;

/// The devices whose own settings the Control Center's Headset and Mouse
/// pages change (#428, #431), built from the tables the service uses —
/// `AsusHeadsetDevice.SettingsModels` and `MouseCatalog` — so the "Device
/// settings" list in docs/supported-devices.md and on the website can't
/// drift from them. `SupportedDevicesTests` compares both with this.
public static class DeviceSettingsCatalog
{
    public const string FromGHelper = "From G-Helper, not yet verified";

    public static IReadOnlyList<SupportedDevice> All()
    {
        var rows = new List<SupportedDevice>();

        rows.AddRange(AsusHeadsetDevice.SettingsModels
            .GroupBy(m => (Name: AsusHeadsetDevice.Models[m.Key], m.Value.NoiseLevels,
                Status: AsusHeadsetDevice.VerifiedSettingsModels.Contains(m.Key) ? LightingCatalog.Verified : LightingCatalog.FromGearLink))
            .OrderBy(g => g.Key.Status == LightingCatalog.Verified ? 0 : 1).ThenBy(g => g.Key.Name)
            .Select(g => new SupportedDevice(g.Key.Name, "Headset", string.Join(", ", g.Select(m => Hex(m.Key)).Order()), g.Key.Status,
                "Sidetone on/off and 0–20; noise reduction " + (g.Key.NoiseLevels > 0 ? "on/off and Low / Medium / High" : "on/off")
                + ", through its 2.4 GHz dongle")));

        // One row per mouse, under ASUS's name (MouseCatalog.AsusNames, #446).
        rows.AddRange(MouseCatalog.Entries
            .GroupBy(e => e.Spec.Name)
            .Select(g =>
            {
                var verified = g.Where(e => MouseCatalog.Verified.Contains((e.ProductId, e.Link))).Select(e => e.Link).ToList();
                var status = verified.Count == 0
                    ? FromGHelper
                    : $"{LightingCatalog.Verified} ({string.Join(", ", verified.Order().Select(Link))})";
                var notes = string.Join(", ", g.Select(e => e.Link).Distinct().Order().Select(Link))
                    + (g.Any(e => e.Spec.Buttons) ? "" : "; no button bindings yet (its own button layout)");
                return new SupportedDevice(g.Key, "Mouse",
                    string.Join(", ", g.Select(e => Hex(e.ProductId)).Distinct().Order()), status, Upper(notes));
            })
            .OrderBy(r => r.Status.StartsWith(LightingCatalog.Verified, StringComparison.Ordinal) ? 0 : 1).ThenBy(r => r.Name));

        return rows;
    }

    /// The "Device settings" section of docs/supported-devices.md.
    public static string Markdown()
    {
        var md = new StringBuilder();
        md.Append("## Device settings\n\n");
        md.Append("The Control Center's **Headset** and **Mouse** pages change these devices' own settings, kept in the device. ");
        md.Append("Mice: DPI levels, polling rate, button response, lift-off, angle snapping and tuning, acceleration, ");
        md.Append("button bindings and energy — as far as the model has them — and which onboard profile each Control Center profile uses. ");
        md.Append(CultureInfo.InvariantCulture, $"**{FromGHelper}**: the model and its settings as G-Helper documents them, read as documentation; ");
        md.Append("the commands are the ones checked on the verified mouse and against ASUS Gear Link.\n\n");
        md.Append("| Device | Type | USB id | Status | Notes |\n|---|---|---|---|---|\n");
        foreach (var row in All())
            md.Append(CultureInfo.InvariantCulture, $"| {row.Name} | {row.Type} | {row.UsbIds} | {row.Status} | {row.Notes} |\n");
        md.Append("\nNot yet: older ROG mice whose commands differ (Gladius II, Strix Impact, Pugio, Chakram / Core, Keris Wireless, ");
        md.Append("Strix Carry, TUF M5 — [#432](https://github.com/dvalfrid/rigstats/issues/432)). ");
        md.Append("To confirm a model, or if one doesn't work: open its page once, then collect diagnostics — the service log ");
        md.Append("(`rigstats-sensor.log`) has a `Mouse: … read …` or `Headset: … read …` line with what the device answered — and send the ZIP ");
        md.Append("with a [device report](https://github.com/dvalfrid/rigstats/issues/new?template=device.yml).\n\n");
        return md.ToString();
    }

    /// The website's device-settings table body, between its markers.
    public static string HtmlRows() => LightingCatalog.HtmlRows(All());

    private static string Link(MouseLink link) => link switch
    {
        MouseLink.Wired => "cable",
        MouseLink.Dongle => "own 2.4 GHz dongle",
        _ => "ROG Omni receiver",
    };

    private static string Upper(string text) => text.Length == 0 ? text : char.ToUpperInvariant(text[0]) + text[1..];

    private static string Hex(ushort id) => $"{id:X4}";
}
