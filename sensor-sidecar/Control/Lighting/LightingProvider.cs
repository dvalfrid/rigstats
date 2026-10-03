using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

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
public sealed class LightingProvider(
    IReadOnlyList<ILightingDevice> devices,
    string unavailableReason,
    Func<string?> conflict,
    bool dryRun) : IControlProvider
{
    private static readonly string[] Effects = ["off", "static", "breathing", "spectrum_cycle"];

    private readonly object _lock = new();
    private AuraPart? _current;

    public string Domain => "aura";

    public CapabilitySet Probe()
    {
        if (devices.Count == 0)
            return new CapabilitySet { Domain = Domain, Supported = false, Reason = unavailableReason };
        if (conflict() is { } other)
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
            Set(aura);
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
        foreach (var device in devices)
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
        if (devices.Count == 0)
            return;
        if (conflict() is { } other)
        {
            SidecarLog.Log($"[rigstats-control] Lighting skipped: {other} controls the lighting.");
            return;
        }
        var (effect, red, green, blue) = Resolve(aura);
        if (dryRun)
        {
            SidecarLog.Log($"[rigstats-control] dry-run: lighting -> {effect} #{red:x2}{green:x2}{blue:x2} on {devices.Count} device(s)");
        }
        else
        {
            // One unplugged device must not keep the others dark; only when
            // every device fails does the profile apply fail. A device another
            // controller owns (Windows Dynamic Lighting) is skipped.
            var failures = new List<string>();
            var targets = devices.Where(d => d.Blocked is null).ToList();
            foreach (var device in targets)
            {
                try
                {
                    device.Apply(effect, red, green, blue);
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

    // ── Pure helpers (unit-tested) ─────────────────────────────────────

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
