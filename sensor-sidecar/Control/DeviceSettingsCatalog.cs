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
                + EqNote(g.Select(m => m.Key))
                + ", through its 2.4 GHz dongle")));

        // Keyboards (#458, #460): one row per model, the Omni receiver's own
        // entry left out (its keyboard is listed under its own name).
        rows.AddRange(AsusKeyboardDevice.Models
            .Where(m => !m.Value.Receiver)
            .GroupBy(m => m.Value.Name)
            .Select(g =>
            {
                var ids = g.Select(m => m.Key).ToList();
                var battery = ids.Any(AsusKeyboardDevice.BatteryModels.Contains);
                var status = ids.Any(AsusKeyboardDevice.VerifiedSettingsModels.Contains) ? LightingCatalog.Verified : LightingCatalog.FromGearLink;
                var notes = "Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back"
                    + (battery ? "; sleep, power saving and the low-battery level" : "");
                return new SupportedDevice(g.Key, "Keyboard", string.Join(", ", ids.Order().Select(Hex)), status, notes);
            })
            .OrderBy(r => r.Status == LightingCatalog.Verified ? 0 : 1).ThenBy(r => r.Name));

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
                    + (g.Any(e => e.Spec.Buttons) ? ""
                        : g.All(e => e.Spec.Frames == MouseFrames.Common) ? "; no button bindings yet (its own button layout)"
                        : "; no button bindings yet (an older model)")
                    + (g.All(e => e.Spec.CanSwitchStage) ? "" : "; the DPI level in use is picked with its DPI button");
                return new SupportedDevice(g.Key, "Mouse",
                    string.Join(", ", g.Select(e => Hex(e.ProductId)).Distinct().Order()), status, Upper(notes));
            })
            .OrderBy(r => r.Status.StartsWith(LightingCatalog.Verified, StringComparison.Ordinal) ? 0 : 1).ThenBy(r => r.Name));

        return rows;
    }

    // "; equalizer on/off and 8 bands (125 Hz – 16 kHz), presets" (#483), or
    // nothing for a model without one.
    private static string EqNote(IEnumerable<ushort> ids)
    {
        var list = ids.ToList();
        if (list.Select(id => AsusHeadsetDevice.EqModels.GetValueOrDefault(id)).FirstOrDefault(l => l is not null) is not { } eq)
            return "";
        var verified = list.Any(AsusHeadsetDevice.VerifiedEqModels.Contains);
        return $"; equalizer on/off and {HeadsetEqCommands.Bands - eq.FirstBand} bands ({HeadsetEqCommands.BandNames[eq.FirstBand]} – 16 kHz), "
            + $"{eq.Presets.Count} presets (G-Helper's tables), one per Control Center profile if wanted"
            + (verified ? "" : " — equalizer not yet verified");
    }

    /// The "Device settings" section of docs/supported-devices.md.
    public static string Markdown()
    {
        var md = new StringBuilder();
        md.Append("## Device settings\n\n");
        md.Append("The Control Center's **Headset**, **Keyboard** and **Mouse** pages change these devices' own settings, kept in the device. ");
        md.Append("Mice: DPI levels, polling rate, button response, lift-off, angle snapping and tuning, acceleration, ");
        md.Append("button bindings and energy — as far as the model has them — and which onboard profile each Control Center profile uses. ");
        md.Append("Keyboards: which onboard profile each Control Center profile uses (Fn + that number on the keyboard switches the profile back), ");
        md.Append("and on wireless models when the keyboard sleeps, what its lighting does when the battery is low and at which level. ");
        md.Append(CultureInfo.InvariantCulture, $"**{FromGHelper}**: the model and its settings as G-Helper documents them, read as documentation; ");
        md.Append("the commands are the ones checked on the verified mouse and against ASUS Gear Link.\n\n");
        md.Append("| Device | Type | USB id | Status | Notes |\n|---|---|---|---|---|\n");
        foreach (var row in All())
            md.Append(CultureInfo.InvariantCulture, $"| {row.Name} | {row.Type} | {row.UsbIds} | {row.Status} | {row.Notes} |\n");
        md.Append("\nThe older models (Gladius II, Strix Impact I / II, Pugio I / II, Chakram / Core, Keris, Strix Carry, TUF Gaming M5) ");
        md.Append("use their own variants of the commands, each as G-Helper documents it (#432). ");
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
