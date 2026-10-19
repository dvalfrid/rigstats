using System.Text.Json.Nodes;
using SensorSidecar.Control.Lighting;

namespace SensorSidecar.Control.Headsets;

/// ASUS headsets (#483): the `headset` profile domain — the equalizer preset
/// a RIGStats profile switches the headsets to. The headsets are the ones
/// the lighting side found (`LightingProvider.HeadsetEqs`), so the dongle's
/// channel is shared, not opened twice. A preset a model doesn't have is
/// left out for that headset.
public sealed class HeadsetProvider(Func<IReadOnlyList<IHeadsetEqDevice>> headsets, bool dryRun)
    : IControlProvider, IAffectsPart
{
    public string Domain => "headset";

    public bool Affects(ProfilePart part) => part.Headset?.EqPreset is not null;

    public CapabilitySet Probe()
    {
        var found = headsets();
        if (found.Count == 0)
            return new CapabilitySet { Domain = Domain, Supported = false, Reason = "No ASUS headset with a known equalizer found." };
        return new CapabilitySet
        {
            Domain = Domain,
            Supported = true,
            Details = new JsonObject
            {
                ["headsets"] = new JsonArray(found.Select(h => (JsonNode)new JsonObject
                {
                    ["id"] = h.Id,
                    ["name"] = h.Name,
                    ["presets"] = new JsonArray(h.EqLimits!.Presets.Select(p => (JsonNode)p.Name).ToArray()),
                }).ToArray()),
            },
        };
    }

    public ValidationResult Validate(ProfilePart part) =>
        part.Headset?.EqPreset is { } name && string.IsNullOrWhiteSpace(name)
            ? ValidationResult.Failure("The equalizer preset needs a name.")
            : ValidationResult.Success();

    /// Each headset's equalizer now (on/off and gains).
    public Snapshot Capture()
    {
        var state = new JsonObject();
        foreach (var headset in headsets())
        {
            if (headset.ReadEq() is { } now)
                state[headset.Id] = new JsonObject
                {
                    ["on"] = now.On,
                    ["gains"] = new JsonArray(now.Gains.Select(g => (JsonNode)g).ToArray()),
                };
        }
        return new Snapshot { Domain = Domain, State = state };
    }

    public void Apply(ProfilePart part)
    {
        if (part.Headset?.EqPreset is not { } name)
            return;
        var all = headsets();
        if (all.Count == 0)
            throw new DomainUnavailableException("No ASUS headset is connected, so its equalizer preset was skipped.");
        // Every headset is tried; one that is off behind its dongle doesn't
        // keep the others from switching. The rest of the profile applies.
        var silent = new List<string>();
        foreach (var headset in all)
        {
            if (HeadsetEqCommands.Preset(headset.EqLimits!, name) is not { } preset)
            {
                SidecarLog.Log($"[rigstats-control] Headset: {headset.Name} has no '{name}' preset; its equalizer is left as it is.");
                continue;
            }
            if (dryRun)
            {
                SidecarLog.Log($"[rigstats-control] dry-run: {headset.Name} -> equalizer preset {preset.Name}");
                continue;
            }
            try
            {
                headset.WriteEq(new HeadsetEq(true, preset.Gains));
                SidecarLog.Log($"[rigstats-control] Headset: {headset.Name} on equalizer preset {preset.Name}.");
            }
            catch (IOException e)
            {
                SidecarLog.Log($"[rigstats-control] Headset: {headset.Name} equalizer not set: {e.Message}");
                silent.Add(headset.Name);
            }
        }
        if (silent.Count > 0)
            throw DomainUnavailableException.Silent(silent);
    }

    public bool Verify(ProfilePart part)
    {
        if (part.Headset?.EqPreset is not { } name || dryRun)
            return true;
        return headsets().All(h =>
            HeadsetEqCommands.Preset(h.EqLimits!, name) is not { } preset
            || h.ReadEq() is not { } now
            || now.SameAs(HeadsetEqCommands.Clamp(new HeadsetEq(true, preset.Gains), h.EqLimits!), h.EqLimits!));
    }

    public void Restore(Snapshot snapshot)
    {
        if (snapshot.Domain != Domain || snapshot.State is not JsonObject state || dryRun)
            return;
        foreach (var headset in headsets())
        {
            if (state[headset.Id] is not JsonObject before)
                continue;
            try
            {
                headset.WriteEq(new HeadsetEq(
                    before["on"]?.GetValue<bool>() ?? false,
                    before["gains"]?.AsArray().Select(g => g!.GetValue<int>()).ToArray() ?? []));
            }
            catch (IOException)
            {
                // Best effort: the headset keeps whichever equalizer it has.
            }
        }
    }

    /// The headset keeps its equalizer — nothing RIGStats holds.
    public string? ReleaseToFirmware() => null;
}
