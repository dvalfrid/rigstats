using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// Which lighting devices this machine has (#192, #212): every driver asked in
/// turn over the HID collections Windows lists, plus the chosen Hue rooms.
/// `LightingProvider` calls it at its first scan and again whenever the HID
/// collections change.
public static class LightingDiscovery
{
    /// Every lighting device found. A headset that is off behind its dongle
    /// goes into the scan's `Retry`, asked again on later rescans.
    public static LightingScan Scan(IReadOnlyList<HidDeviceInfo> hid, HueLink hue)
    {
        var devices = new List<ILightingDevice>();
        if (AuraController.TryOpen(out var reason) is { } motherboard)
            devices.Add(motherboard);
        // ASUS Aura monitors and the monitor light bar (#212).
        devices.AddRange(AsusMonitorDevice.Discover(hid));
        // ASUS TUF-protocol keyboards — the ROG Azoth X via the Omni receiver
        // or cable, the ROG Falchion Ace HFX (#213).
        var keyboards = AsusKeyboardDevice.Discover(hid);
        devices.AddRange(keyboards);
        // ASUS headsets on the GearLink protocol — the ROG Delta II via its
        // dongle. A headset switched off is asked again on later rescans.
        var silent = new List<HidDeviceInfo>();
        devices.AddRange(AsusHeadsetDevice.Discover(hid, silent));
        // Razer mice and laptop keyboards on Razer's own protocol (#510);
        // models that show a LampArray are left to LampArrayDevice below.
        devices.AddRange(RazerDevice.Discover(hid));
        // HyperX Alloy Origins Core keyboards (#511) and SteelSeries Siberia
        // headsets (#512), from OpenRGB's documentation.
        devices.AddRange(HyperXKeyboardDevice.Discover(hid));
        devices.AddRange(SteelSeriesHeadsetDevice.Discover(hid));
        // Any HID LampArray device (Windows Dynamic Lighting standard), any
        // brand — except keyboards already driven by their own protocol above.
        devices.AddRange(LampArrayDevice.Discover(
            AsusKeyboardDevice.WithoutDirectKeyboards(hid, keyboards), LampArrayDevice.WindowsDynamicLightingOn));
        // Philips Hue rooms and zones chosen to follow the rig — over the
        // network, from the paired bridge.
        devices.AddRange(hue.Devices());
        return new LightingScan(devices, devices.Count > 0 ? "" : reason, RetryHeadsets(silent));
    }

    /// For the diagnostics export: what was found, and every HID collection
    /// seen, so unknown devices can be supported from a user's export.
    public static void WriteDiagnostics(string path, IReadOnlyList<HidDeviceInfo> hid,
        IReadOnlyList<ILightingDevice> devices, string reason) =>
        LightingDiagnostics.Write(path, LightingDiagnostics.Build(hid, devices, reason,
            OtherSoftware.LightingTaken(), LampArrayDevice.WindowsDynamicLightingOn(), ProbeAsus(hid)));

    /// Asks the silent headset dongles again, until every headset has answered.
    private static Func<LightingScan>? RetryHeadsets(List<HidDeviceInfo> silent) => silent.Count == 0 ? null : () =>
    {
        var stillSilent = new List<HidDeviceInfo>();
        var found = AsusHeadsetDevice.Discover(silent, stillSilent);
        return new LightingScan(found, "", RetryHeadsets(stillSilent));
    };

    /// The ASUS receivers' read-only replies for the diagnostics file; never
    /// fails the discovery it is written after.
    private static JsonArray? ProbeAsus(IReadOnlyList<HidDeviceInfo> hid)
    {
        try
        {
            return OmniMouse.Probe(hid);
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] Lighting: ASUS receiver probe failed: {e.Message}");
            return null;
        }
    }
}
