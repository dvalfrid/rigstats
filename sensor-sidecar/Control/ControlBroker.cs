namespace SensorSidecar.Control;

/// Runs every profile change as one transaction: Validate all affected
/// domains → Capture a snapshot from each → Apply in a fixed cross-domain
/// order → Verify by readback → on any failure, Restore every already-applied
/// provider in reverse order and return one consolidated result. Never
/// applies partially — either the whole profile takes effect or none of it
/// does. See `docs/control-architecture.md`, "ControlBroker".
public sealed class ControlBroker(IEnumerable<IControlProvider> providers)
{
    // Fixed apply order from the design doc — later phases add the
    // providers for the domains that don't exist yet in phase 0; anything
    // with no registered provider is simply skipped.
    private static readonly string[] DomainOrder =
        ["power_plan", "cpu_limit", "curve_opt", "gpu", "fan", "aura"];

    private readonly Dictionary<string, IControlProvider> _byDomain =
        providers.ToDictionary(p => p.Domain);

    // Doc: "Transactions are serialised — there is never more than one in flight."
    private readonly SemaphoreSlim _transactionLock = new(1, 1);

    public async Task<ApplyResult> ApplyProfileAsync(Profile profile, CancellationToken ct)
    {
        await _transactionLock.WaitAsync(ct);
        try
        {
            return Apply(profile);
        }
        finally
        {
            _transactionLock.Release();
        }
    }

    private ApplyResult Apply(Profile profile)
    {
        var affected = DomainOrder
            .Where(HasPart(profile.Part))
            .Select(domain => _byDomain.TryGetValue(domain, out var p) ? p : null)
            .Where(p => p is not null)
            .Select(p => p!)
            .ToList();

        if (affected.Count == 0)
            return ApplyResult.Success(profile.Id);

        // 1. Validate every affected domain up front — reject before touching
        //    anything if any single part is invalid.
        foreach (var provider in affected)
        {
            var validation = provider.Validate(profile.Part);
            if (!validation.Ok)
            {
                return ApplyResult.Failure(
                    profile.Id,
                    $"Profile {profile.Name} not applied: {validation.Reason ?? $"{provider.Domain} rejected the requested value."}");
            }
        }

        // 2. Snapshot every affected domain before changing anything, so a
        //    failure partway through can be fully rolled back.
        var snapshots = new List<(IControlProvider Provider, Snapshot Snapshot)>();
        foreach (var provider in affected)
            snapshots.Add((provider, provider.Capture()));

        // 3. Apply in the fixed order, verifying each as we go. On the first
        //    failure, restore everything already applied, in reverse order.
        var applied = new List<IControlProvider>();
        try
        {
            foreach (var provider in affected)
            {
                provider.Apply(profile.Part);
                applied.Add(provider);
                if (!provider.Verify(profile.Part))
                    throw new ControlApplyException($"{provider.Domain} did not take effect after applying.");
            }
        }
        catch (Exception e)
        {
            for (var i = applied.Count - 1; i >= 0; i--)
            {
                var snapshot = snapshots.First(s => s.Provider == applied[i]).Snapshot;
                try
                {
                    applied[i].Restore(snapshot);
                }
                catch
                {
                    // Best-effort rollback — a restore failure doesn't change
                    // the outcome (the transaction already failed), but must
                    // not stop the remaining providers from being restored.
                }
            }
            return ApplyResult.Failure(profile.Id, $"Profile {profile.Name} not applied: {e.Message}");
        }

        return ApplyResult.Success(profile.Id);
    }

    private static Func<string, bool> HasPart(ProfilePart part) => domain => domain switch
    {
        "power_plan" => part.PowerPlan is not null,
        "cpu_limit" => part.CpuLimit is not null,
        "curve_opt" => part.CurveOpt is not null,
        "gpu" => part.Gpu is not null,
        "fan" => part.Fan is not null,
        "aura" => part.Aura is not null,
        _ => false,
    };
}

/// Raised internally when a provider's `Verify` fails right after `Apply` —
/// caught by `ControlBroker.Apply` to trigger rollback; never escapes it.
internal sealed class ControlApplyException(string message) : Exception(message);
