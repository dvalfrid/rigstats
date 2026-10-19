using System.Text.Json.Nodes;
using SensorSidecar.Control.Lighting;

namespace SensorSidecar.Control.Keyboards;

/// ASUS keyboards (#458): the `keyboard` profile domain — which of the
/// keyboard's own onboard profiles a RIGStats profile uses. The keyboards are
/// the ones the lighting side found (`LightingProvider.Keyboards`), so the
/// command channel is shared, not opened twice.
public sealed class KeyboardProvider(Func<IReadOnlyList<IKeyboardProfileDevice>> keyboards, bool dryRun)
    : IControlProvider, IAffectsPart
{
    public string Domain => "keyboard";

    public bool Affects(ProfilePart part) => part.Keyboard?.Profile is not null;

    public CapabilitySet Probe()
    {
        // From what discovery read, not a live question: a wireless keyboard
        // asleep at that moment would otherwise hide the page.
        var found = keyboards()
            .Select(k => (Keyboard: k, Profiles: k.FoundProfiles))
            .Where(k => k.Profiles is { Count: > 1 })
            .ToList();
        if (found.Count == 0)
            return new CapabilitySet { Domain = Domain, Supported = false, Reason = "No ASUS keyboard with onboard profiles found." };
        return new CapabilitySet
        {
            Domain = Domain,
            Supported = true,
            Details = new JsonObject
            {
                ["keyboards"] = new JsonArray(found.Select(k => (JsonNode)new JsonObject
                {
                    ["id"] = k.Keyboard.Id,
                    ["name"] = k.Keyboard.Name,
                    ["profiles"] = k.Profiles!.Count,
                }).ToArray()),
            },
        };
    }

    public ValidationResult Validate(ProfilePart part) =>
        part.Keyboard?.Profile is { } p && p < 1
            ? ValidationResult.Failure("The keyboard profile must be 1 or more.")
            : ValidationResult.Success();

    /// The onboard profile each keyboard uses now.
    public Snapshot Capture()
    {
        var state = new JsonObject();
        foreach (var keyboard in keyboards())
        {
            if (keyboard.ReadProfiles() is { } now)
                state[keyboard.Id] = now.Current;
        }
        return new Snapshot { Domain = Domain, State = state };
    }

    public void Apply(ProfilePart part)
    {
        if (part.Keyboard?.Profile is not { } profile)
            return;
        var all = keyboards();
        if (all.Count == 0)
            throw new DomainUnavailableException("No ASUS keyboard is connected, so its profile was skipped.");
        // Every keyboard is tried; one that doesn't answer (asleep behind its
        // receiver) doesn't keep the others from switching (#484). The rest
        // of the profile still applies.
        var silent = new List<string>();
        foreach (var keyboard in all)
        {
            if (dryRun)
            {
                SidecarLog.Log($"[rigstats-control] dry-run: {keyboard.Name} -> onboard profile {profile}");
                continue;
            }
            try
            {
                keyboard.SwitchProfile(profile);
                SidecarLog.Log($"[rigstats-control] Keyboard: {keyboard.Name} on onboard profile {profile}.");
            }
            catch (IOException e)
            {
                silent.Add($"{keyboard.Name} ({e.Message})");
            }
        }
        if (silent.Count > 0)
            throw new DomainUnavailableException($"{string.Join(", ", silent)} didn't answer, so its profile was skipped.");
    }

    public bool Verify(ProfilePart part)
    {
        if (part.Keyboard?.Profile is not { } profile || dryRun)
            return true;
        return keyboards().All(k => k.ReadProfiles() is not { } now || now.Current == Math.Clamp(profile, 1, now.Count));
    }

    public void Restore(Snapshot snapshot)
    {
        if (snapshot.Domain != Domain || snapshot.State is not JsonObject state || dryRun)
            return;
        foreach (var keyboard in keyboards())
        {
            if (state[keyboard.Id]?.GetValue<int>() is { } profile)
            {
                try
                {
                    keyboard.SwitchProfile(profile);
                }
                catch (IOException)
                {
                    // Best effort: the keyboard keeps whichever profile it has.
                }
            }
        }
    }

    /// The keyboard keeps its onboard profile — nothing RIGStats holds.
    public string? ReleaseToFirmware() => null;
}
