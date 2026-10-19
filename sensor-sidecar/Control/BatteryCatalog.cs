using System.Globalization;
using System.Text;
using SensorSidecar.Control.Lighting;

namespace SensorSidecar.Control;

/// The devices whose battery RIGStats shows, built from the code that reads
/// them — the ROG model tables, the Logitech receiver table — so the
/// "Battery" list in docs/supported-devices.md and on the website can't drift
/// from it. `SupportedDevicesTests` compares both files with what this
/// produces.
public static class BatteryCatalog
{
    public const string FromSolaar = "From Solaar, not yet verified";

    /// Batteries seen on hardware for models whose lighting isn't verified:
    /// the ROG Azoth (2.4 GHz) and the Harpe Ace Mini on a second Omni
    /// receiver (2026-10-19, 64 % and 53 %, #296).
    public static readonly IReadOnlySet<ushort> BatteryVerified = new HashSet<ushort> { 0x1A85, 0x1B65 };

    public static IReadOnlyList<SupportedDevice> All()
    {
        var rows = new List<SupportedDevice>();

        rows.AddRange(AsusHeadsetDevice.Models
            .GroupBy(m => (Name: m.Value, Status: AsusHeadsetDevice.VerifiedModels.Contains(m.Key) ? LightingCatalog.Verified : LightingCatalog.FromGearLink))
            .OrderBy(g => g.Key.Status == LightingCatalog.Verified ? 0 : 1).ThenBy(g => g.Key.Name)
            .Select(g => new SupportedDevice(g.Key.Name, "Headset", string.Join(", ", g.Select(m => Hex(m.Key)).Order()),
                g.Key.Status, "Battery and charging, through its 2.4 GHz dongle")));

        rows.AddRange(AsusKeyboardDevice.Models
            .Where(m => AsusKeyboardDevice.BatteryModels.Contains(m.Key))
            .GroupBy(m => (m.Value.Name,
                Status: m.Value.Verified || BatteryVerified.Contains(m.Key) ? LightingCatalog.Verified : m.Value.GearLink ? LightingCatalog.FromGearLink : LightingCatalog.FromOpenRgb))
            .OrderBy(g => g.Key.Status == LightingCatalog.Verified ? 0 : 1).ThenBy(g => g.Key.Name)
            .Select(g => new SupportedDevice(g.Key.Name, "Keyboard", string.Join(", ", g.Select(m => Hex(m.Key)).Order()),
                g.Key.Status, "Battery and charging")));

        rows.AddRange(OmniMouse.Models
            .OrderByDescending(m => m.Value.Verified || BatteryVerified.Contains(m.Key)).ThenBy(m => m.Value.Name)
            .Select(m => new SupportedDevice(m.Value.Name, "Mouse", Hex(m.Key),
                m.Value.Verified || BatteryVerified.Contains(m.Key) ? LightingCatalog.Verified : LightingCatalog.FromGearLink,
                "Battery and charging, through the ROG Omni receiver")));

        rows.Add(new SupportedDevice(
            "Any Bluetooth device whose battery Windows shows (Settings › Bluetooth & devices), any brand",
            "Mouse, keyboard, headset, other", "— (Bluetooth)",
            LightingCatalog.Verified + " (Logitech MX Anywhere 3 for Mac, Satechi M1 Mouse)",
            "Battery only: Windows has no charging state for Bluetooth devices. Nothing is sent to the device"));

        rows.Add(new SupportedDevice(
            "Logitech devices on Bluetooth", "Mouse, keyboard", "— (Bluetooth)",
            LightingCatalog.Verified + " (MX Anywhere 3 for Mac, MX Keys Mini, MX Mechanical Mini for Mac)",
            "Battery and charging, asked in Logitech's HID++ protocol"));

        rows.AddRange(new[] { LogitechReceiver.Family.Bolt, LogitechReceiver.Family.Unifying }
            .Select(family => (Family: family, Ids: LogitechReceiver.ProductIds.Where(p => LogitechReceiver.FamilyOf(p) == family).Order().ToList()))
            .Select(r => new SupportedDevice(
                $"Logitech devices on a {r.Family} receiver", "Mouse, keyboard",
                string.Join(", ", r.Ids.Select(Hex)) + " (receiver)",
                r.Ids.Any(LogitechReceiver.Verified.Contains)
                    ? $"{LightingCatalog.Verified} ({string.Join(", ", r.Ids.Where(LogitechReceiver.Verified.Contains).Select(id => LogitechReceiver.VerifiedWith[id]))})"
                    : FromSolaar,
                "Battery and charging, asked in Logitech's HID++ protocol, up to six devices per receiver")));

        return rows;
    }

    /// The "Battery" section of docs/supported-devices.md.
    public static string Markdown()
    {
        var md = new StringBuilder();
        md.Append("## Battery\n\n");
        md.Append("The PERIPHERALS panel and the tray icon's hover card show the battery of these devices, read every 20 s. ");
        md.Append(CultureInfo.InvariantCulture, $"**{FromSolaar}**: built from Solaar's implementation of Logitech's protocol, read as documentation.\n\n");
        md.Append("| Device | Type | USB id | Status | Notes |\n|---|---|---|---|---|\n");
        foreach (var row in All())
            md.Append(CultureInfo.InvariantCulture, $"| {row.Name} | {row.Type} | {row.UsbIds} | {row.Status} | {row.Notes} |\n");
        md.Append("\nNot read: ROG mice on their own 2.4 GHz dongle or cable, Logitech Nano and Lightspeed receivers, ");
        md.Append("and Logitech devices that report only a coarse level, not a percentage. A device that doesn't show, or one that works: ");
        md.Append("the service log (`rigstats-sensor.log`, in the diagnostics ZIP) has a line starting with `Battery:` for every device it found — ");
        md.Append("send it with a [device report](https://github.com/dvalfrid/rigstats/issues/new?template=device.yml).\n");
        return md.ToString();
    }

    /// The website's battery table body, between the battery-devices markers.
    public static string HtmlRows() => LightingCatalog.HtmlRows(All());

    private static string Hex(ushort id) => $"{id:X4}";
}
