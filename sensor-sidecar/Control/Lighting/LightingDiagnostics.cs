using System.Text.Json;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// `lighting-devices.json` for the diagnostics export (#212): every device
/// RIGStats drives with what a fixture or fix needs, plus a scan of every
/// HID collection on the machine — so support for a board, monitor or
/// keyboard RIGStats doesn't know yet can be added from a user's export,
/// without the hardware. HID paths and serial numbers are left out: they
/// identify the machine and add nothing.
public static class LightingDiagnostics
{
    public static JsonObject Build(
        IReadOnlyList<HidDeviceInfo> hid,
        IReadOnlyList<ILightingDevice> devices,
        string unavailableReason,
        string? conflict,
        bool dynamicLightingOn,
        JsonArray? asusProbes = null) => new()
        {
            // Read-only replies from ASUS receivers and their paired devices
            // (OmniMouse.Probe): what adding a new mouse needs.
            ["asus_probes"] = asusProbes,
            ["written_utc"] = DateTimeOffset.UtcNow.ToString("O"),
            ["conflict"] = conflict,
            ["windows_dynamic_lighting_on"] = dynamicLightingOn,
            ["unavailable_reason"] = devices.Count == 0 ? unavailableReason : null,
            ["devices"] = new JsonArray(devices.Select(d =>
            {
                var entry = new JsonObject
                {
                    ["id"] = d.Id,
                    ["name"] = d.Name,
                    ["kind"] = d.Kind,
                    ["blocked"] = d.Blocked,
                    ["zones"] = new JsonArray(d.Zones.Select(z => (JsonNode)$"{z.Id} ({z.Leds})").ToArray()),
                };
                foreach (var (key, value) in d.Diagnostics())
                    entry[key] = value?.DeepClone();
                return (JsonNode)entry;
            }).ToArray()),
            ["hid_scan"] = new JsonArray(hid
                .OrderBy(h => h.VendorId).ThenBy(h => h.ProductId).ThenBy(h => h.Interface).ThenBy(h => h.UsagePage)
                .Select(h => (JsonNode)new JsonObject
                {
                    ["vendor_id"] = $"{h.VendorId:X4}",
                    ["product_id"] = $"{h.ProductId:X4}",
                    ["interface"] = h.Interface,
                    ["usage_page"] = $"{h.UsagePage:X4}",
                    ["usage"] = $"{h.Usage:X4}",
                    ["input_len"] = h.InputReportLength,
                    ["output_len"] = h.OutputReportLength,
                    ["feature_len"] = h.FeatureReportLength,
                    ["product"] = h.Product,
                    ["known_as"] = KnownAs(h),
                }).ToArray()),
        };

    /// What RIGStats takes a HID collection for, or null when it doesn't
    /// know it — the null ones are the candidates for new support.
    public static string? KnownAs(HidDeviceInfo h)
    {
        if (h.UsagePage == LampArrayDevice.LightingPage)
            return "lamp_array";
        if (h.VendorId == AuraUsb.AsusVendorId && AsusHeadsetDevice.Model(h.ProductId) is not null && h.UsagePage == 0xFF00)
            return "asus_headset";
        if (h.VendorId == AuraUsb.AsusVendorId && AsusKeyboardDevice.Model(h.ProductId) is { } keyboard
            && (keyboard.Receiver ? h.UsagePage is >= 0xFF00 and <= 0xFF02 : h.UsagePage == 0xFF00))
            return "asus_keyboard_channel";
        if (h.VendorId != AuraUsb.AsusVendorId || h.UsagePage != AuraUsb.AuraUsagePage)
            return null;
        if (AuraUsb.Family(h.ProductId) is { } family)
            return $"aura_{family.ToString().ToLowerInvariant()}";
        return AsusMonitorDevice.Model(h.ProductId) is { } model ? $"aura_{model.Kind}" : null;
    }

    public static void Write(string path, JsonObject document)
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.WriteAllText(path, document.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] Lighting diagnostics not written: {e.Message}");
        }
    }
}
