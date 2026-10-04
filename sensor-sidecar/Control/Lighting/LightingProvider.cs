using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// What one discovery found. `Retry` asks again only the devices that
/// didn't answer (a headset off behind its dongle), or is null.
public sealed record LightingScan(IReadOnlyList<ILightingDevice> Devices, string UnavailableReason, Func<LightingScan>? Retry = null);

/// `IControlProvider` for lighting (#192, Control Center phase 5). Aura
/// Sync: a profile's one effect goes to every `ILightingDevice` found — the
/// motherboard's ASUS Aura USB controller today, monitors and peripherals
/// later, without changing profiles, the pipe or the UI. Each controller is
/// found by its known USB ids and describes its own zones, so no board is
/// hard-coded. Lighting is low-risk (worst case: wrong colours), so every
/// known controller is offered, not only hardware-verified ones.
///
/// Devices can't report their current effect, so `Verify` trusts a
/// successful write and `Capture` returns what this service last set. A
/// profile without a lighting part leaves the lights alone.
///
/// Devices come and go (a keyboard switched from its receiver to the cable,
/// a monitor turned on), so the list is looked for again when the Control
/// Center asks for capabilities and before a profile applies — at most every
/// two seconds, never per live-preview step, and the devices are only probed
/// again when the set of HID collections changed. A device still there is
/// kept as it is; a new one gets the current lighting at once. A device that
/// can sit silent behind a plugged-in dongle (a headset switched off) is
/// asked again on each rescan through the scan's `Retry`, alone.
public sealed class LightingProvider : IControlProvider
{
    private static readonly string[] Effects = ["off", "static", "breathing", "spectrum_cycle"];
    private const long RescanIntervalMs = 2000;

    private readonly Func<IReadOnlyList<HidDeviceInfo>> _enumerate;
    private readonly Func<IReadOnlyList<HidDeviceInfo>, LightingScan> _discover;
    private readonly Func<string?> _conflict;
    private readonly bool _dryRun;
    private readonly Action<IReadOnlyList<HidDeviceInfo>, IReadOnlyList<ILightingDevice>, string>? _scanned;
    private readonly Func<long> _clock;

    private readonly object _lock = new();
    private readonly object _scanLock = new();
    private AuraPart? _current;
    private volatile IReadOnlyList<ILightingDevice> _devices = [];
    private volatile string _unavailableReason = "";
    private string? _fingerprint;
    private Func<LightingScan>? _retry;
    private long _lastScan;

    /// `enumerate` lists the HID collections, `discover` turns them into
    /// devices; `scanned` sees every completed discovery (the diagnostics
    /// file). The first discovery runs here, logged in full.
    public LightingProvider(
        Func<IReadOnlyList<HidDeviceInfo>> enumerate,
        Func<IReadOnlyList<HidDeviceInfo>, LightingScan> discover,
        Func<string?> conflict,
        bool dryRun,
        Action<IReadOnlyList<HidDeviceInfo>, IReadOnlyList<ILightingDevice>, string>? scanned = null,
        Func<long>? clock = null)
    {
        _enumerate = enumerate;
        _discover = discover;
        _conflict = conflict;
        _dryRun = dryRun;
        _scanned = scanned;
        _clock = clock ?? (() => Environment.TickCount64);
        Rescan(force: true);
    }

    /// A fixed device list (tests).
    public LightingProvider(IReadOnlyList<ILightingDevice> devices, string unavailableReason, Func<string?> conflict, bool dryRun)
        : this(() => [], _ => new LightingScan(devices, unavailableReason), conflict, dryRun)
    {
    }

    public IReadOnlyList<ILightingDevice> Devices => _devices;

    public string Domain => "aura";

    /// Looks for added and removed devices (see the class comment).
    public void Rescan(bool force = false)
    {
        lock (_scanLock)
        {
            var now = _clock();
            if (!force && _fingerprint is not null && now - _lastScan < RescanIntervalMs)
                return;
            _lastScan = now;

            IReadOnlyList<HidDeviceInfo> hid;
            try
            {
                hid = _enumerate();
            }
            catch (Exception e)
            {
                SidecarLog.Log($"[rigstats-control] Lighting: device scan failed: {e.Message}");
                return;
            }
            var fingerprint = string.Join("\n", hid.Select(h => h.Path).Order(StringComparer.OrdinalIgnoreCase));
            var first = _fingerprint is null;
            var retryOnly = !first && fingerprint == _fingerprint;
            if (retryOnly && _retry is null)
                return;
            _fingerprint = fingerprint;

            LightingScan scan;
            using (first ? null : LightingLog.Quiet())
                scan = retryOnly ? _retry!() : _discover(hid);
            _retry = scan.Retry;
            if (retryOnly && scan.Devices.Count == 0)
                return;

            // Keep the instance already driving a device (its handle, its
            // running effect); the fresh duplicate is closed. A retry only
            // adds: the devices it didn't ask stay.
            var old = _devices;
            var merged = retryOnly ? old.ToList() : new List<ILightingDevice>();
            var added = new List<ILightingDevice>();
            foreach (var device in scan.Devices)
            {
                if (old.FirstOrDefault(o => o.Id == device.Id) is { } kept)
                {
                    if (!retryOnly)
                        merged.Add(kept);
                    if (!ReferenceEquals(kept, device))
                        (device as IDisposable)?.Dispose();
                }
                else
                {
                    merged.Add(device);
                    added.Add(device);
                }
            }
            var gone = old.Where(o => !merged.Contains(o)).ToList();
            _devices = merged;
            if (!retryOnly || merged.Count > 0)
                _unavailableReason = merged.Count > 0 ? "" : scan.UnavailableReason;

            foreach (var device in gone)
                (device as IDisposable)?.Dispose();
            if (!first && (added.Count > 0 || gone.Count > 0))
            {
                SidecarLog.Log("[rigstats-control] Lighting devices changed: " + string.Join(", ",
                    added.Select(d => $"+ {d.Name}").Concat(gone.Select(d => $"- {d.Name}"))) + ".");
            }
            _scanned?.Invoke(hid, merged, _unavailableReason);
            if (!first)
                ApplyCurrent(added);
        }
    }

    /// Gives newly found devices the lighting the others already show.
    private void ApplyCurrent(IReadOnlyList<ILightingDevice> added)
    {
        AuraPart? current;
        lock (_lock)
            current = _current;
        if (current is null || added.Count == 0 || _dryRun || _conflict() is not null)
            return;
        foreach (var device in added.Where(d => d.Blocked is null && Touches(d, current)))
        {
            try
            {
                Write(device, current);
            }
            catch (Exception e)
            {
                SidecarLog.Log($"[rigstats-control] Lighting on {device.Name} failed: {e.Message}");
            }
        }
    }

    public CapabilitySet Probe()
    {
        Rescan();
        var devices = _devices;
        if (devices.Count == 0)
            return new CapabilitySet { Domain = Domain, Supported = false, Reason = _unavailableReason };
        if (_conflict() is { } other)
        {
            // Flagged, so the UI explains it instead of hiding the tab.
            return new CapabilitySet
            {
                Domain = Domain,
                Supported = false,
                Reason = $"{other} controls the lighting. Close it (or turn off its lighting) to control lighting here.",
                Details = new JsonObject { ["conflict"] = true },
            };
        }
        return new CapabilitySet
        {
            Domain = Domain,
            Supported = true,
            Details = new JsonObject
            {
                ["effects"] = new JsonArray(Effects.Select(e => (JsonNode)e).ToArray()),
                ["devices"] = new JsonArray(devices.Select(d => (JsonNode)new JsonObject
                {
                    ["id"] = d.Id,
                    ["name"] = d.Name,
                    ["kind"] = d.Kind,
                    ["firmware"] = d.Firmware,
                    ["blocked"] = d.Blocked,
                    ["lamp"] = d is ILampDevice { HasLamp: true },
                    // What the lamp shows now — the tray or its own button may
                    // have switched it since the profile set it.
                    ["lamp_on"] = d is ILampDevice { HasLamp: true } lamp ? LampOnOrNull(lamp) : null,
                    ["zones"] = new JsonArray(d.Zones.Select(z => (JsonNode)new JsonObject
                    {
                        ["id"] = z.Id,
                        ["name"] = z.Name,
                        ["addressable"] = z.Addressable,
                        ["leds"] = z.Leds,
                    }).ToArray()),
                }).ToArray()),
            },
        };
    }

    public ValidationResult Validate(ProfilePart part)
    {
        if (part.Aura is not { } aura)
            return ValidationResult.Success();
        if (aura.Effect is { } effect && ParseEffect(effect) is null)
            return ValidationResult.Failure($"Unknown lighting effect '{effect}'.");
        if (aura.Color is { } color && AuraUsb.ParseColor(color) is null)
            return ValidationResult.Failure($"Lighting colour '{color}' must look like #ff0033.");
        return ValidationResult.Success();
    }

    public Snapshot Capture()
    {
        lock (_lock)
            return new Snapshot { Domain = Domain, State = _current is null ? null : JsonSerializer.SerializeToNode(_current, ControlJson.Options) };
    }

    public void Apply(ProfilePart part)
    {
        if (part.Aura is { } aura)
        {
            Rescan();
            Set(aura);
        }
    }

    /// A lighting write can't be read back; a write that didn't throw counts.
    public bool Verify(ProfilePart part) => true;

    public void Restore(Snapshot snapshot)
    {
        if (snapshot.Domain == Domain && snapshot.State?.Deserialize<AuraPart>(ControlJson.Options) is { } previous)
            Set(previous);
    }

    /// Each device goes back to its own effect where it has one (LampArray
    /// devices); the Aura controllers keep the last effect until a cold boot.
    public void ReleaseToFirmware()
    {
        foreach (var device in _devices)
        {
            try
            {
                device.Release();
            }
            catch (Exception e)
            {
                SidecarLog.Log($"[rigstats-control] Lighting release on {device.Name} failed: {e.Message}");
            }
        }
    }

    /// Applies `aura` at once, outside any transaction — the Lighting tab's
    /// live preview while a colour is being picked.
    public void Preview(AuraPart aura)
    {
        if (Validate(new ProfilePart { Aura = aura }).Ok)
            Set(aura);
    }

    private void Set(AuraPart aura)
    {
        var devices = _devices;
        if (devices.Count == 0)
            return;
        if (_conflict() is { } other)
        {
            SidecarLog.Log($"[rigstats-control] Lighting skipped: {other} controls the lighting.");
            return;
        }
        if (_dryRun)
        {
            var (effect, red, green, blue) = Resolve(aura);
            var lamp = aura.Lamp is { } l ? $", lamp {Lamp.Channels(l)}" : "";
            SidecarLog.Log($"[rigstats-control] dry-run: lighting -> {(SetsRgb(aura) ? $"{effect} #{red:x2}{green:x2}{blue:x2}" : "RGB as is")}{lamp} on {devices.Count} device(s)");
        }
        else
        {
            // One unplugged device must not keep the others dark; only when
            // every device fails does the profile apply fail. A device another
            // controller owns (Windows Dynamic Lighting) is skipped.
            var failures = new List<string>();
            var targets = devices.Where(d => d.Blocked is null && Touches(d, aura)).ToList();
            foreach (var device in targets)
            {
                try
                {
                    Write(device, aura);
                }
                catch (Exception e)
                {
                    failures.Add($"{device.Name}: {e.Message}");
                    SidecarLog.Log($"[rigstats-control] Lighting on {device.Name} failed: {e.Message}");
                }
            }
            if (targets.Count > 0 && failures.Count == targets.Count)
                throw new InvalidOperationException($"Lighting not set ({string.Join("; ", failures)}).");
        }
        lock (_lock)
            _current = aura;
    }

    /// The tray's lamp toggle: every lamp off when any is lit, else every
    /// lamp on again as it last was. Reads the lamps first — their own
    /// buttons switch them too. Returns whether they are on now, or null
    /// when there is no lamp. Not saved in any profile.
    public bool? ToggleLamp()
    {
        Rescan();
        var lamps = _devices.OfType<ILampDevice>().Where(d => d.HasLamp).ToList();
        if (lamps.Count == 0)
            return null;
        var on = !lamps.Any(l => l.LampOn());
        if (_dryRun)
        {
            SidecarLog.Log($"[rigstats-control] dry-run: lamp -> {(on ? "on" : "off")}");
            return on;
        }
        foreach (var lamp in lamps)
            lamp.SwitchLamp(on);
        return on;
    }

    private static bool? LampOnOrNull(ILampDevice lamp)
    {
        try
        {
            return lamp.LampOn();
        }
        catch (Exception)
        {
            return null; // unplugged or busy: unknown, not an error for Probe
        }
    }

    /// The RGB effect (unless the part only sets a lamp), then the lamp.
    private static void Write(ILightingDevice device, AuraPart aura)
    {
        if (SetsRgb(aura))
        {
            var (effect, red, green, blue) = Resolve(aura);
            device.Apply(effect, red, green, blue);
        }
        if (aura.Lamp is { } lamp && device is ILampDevice { HasLamp: true } lampDevice)
            lampDevice.SetLamp(lamp);
    }

    private static bool Touches(ILightingDevice device, AuraPart aura) =>
        SetsRgb(aura) || (aura.Lamp is not null && device is ILampDevice { HasLamp: true });

    // ── Pure helpers (unit-tested) ─────────────────────────────────────

    /// A part with only a lamp leaves the RGB as it is.
    public static bool SetsRgb(AuraPart aura) => aura.Effect is not null || aura.Lamp is null;

    public static AuraEffect? ParseEffect(string? effect) => effect switch
    {
        "off" => AuraEffect.Off,
        "static" => AuraEffect.Static,
        "breathing" => AuraEffect.Breathing,
        "spectrum_cycle" => AuraEffect.SpectrumCycle,
        _ => null,
    };

    /// The effect (default static) and the colour (default white) scaled by
    /// brightness (default full).
    public static (AuraEffect Effect, byte Red, byte Green, byte Blue) Resolve(AuraPart aura)
    {
        var effect = ParseEffect(aura.Effect) ?? AuraEffect.Static;
        var (r, g, b) = AuraUsb.ParseColor(aura.Color) ?? ((byte)255, (byte)255, (byte)255);
        (r, g, b) = AuraUsb.Scale(r, g, b, aura.Brightness ?? 1.0);
        return (effect, r, g, b);
    }

    /// What holds the lighting besides us, or null: Armoury Crate's
    /// LightingService (or its app) or OpenRGB rewrite the devices themselves,
    /// so the two would fight over every change.
    public static string? DetectConflict()
    {
        try
        {
            if (Process.GetProcessesByName("LightingService").Length > 0)
                return "Armoury Crate (LightingService)";
            if (Process.GetProcessesByName("ArmouryCrate").Length > 0)
                return "Armoury Crate";
            // OpenRGB drives the same devices (and redraws them constantly).
            if (Process.GetProcessesByName("OpenRGB").Length > 0)
                return "OpenRGB";
        }
        catch
        {
            // Can't tell — don't block lighting over it.
        }
        return null;
    }
}
