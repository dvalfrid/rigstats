namespace SensorSidecar.Control;

/// Runs every profile change as one transaction: Validate all affected
/// domains → Capture a snapshot from each → Apply in a fixed cross-domain
/// order → Verify by readback → on any failure, Restore every already-applied
/// provider in reverse order and return one consolidated result. Never
/// applies partially — either the whole profile takes effect or none of it
/// does. See `docs/control-architecture.md`, "ControlBroker".
///
/// A `preview` is the same transaction with its snapshots kept: unless
/// `EndPreviewAsync(keep: true)` arrives within the timeout, the service
/// restores them by itself — so a UI that crashed or a limit that made the
/// machine unusable to click "Keep" on is undone without the UI.
public sealed class ControlBroker(IEnumerable<IControlProvider> providers)
{
    // Fixed apply order from the design doc — later phases add the
    // providers for the domains that don't exist yet; anything with no
    // registered provider is simply skipped.
    private static readonly string[] DomainOrder =
        ["power_plan", "cpu_limit", "curve_opt", "gpu", "fan", "aura"];

    private readonly Dictionary<string, IControlProvider> _byDomain =
        providers.ToDictionary(p => p.Domain);

    // Doc: "Transactions are serialised — there is never more than one in flight."
    private readonly SemaphoreSlim _transactionLock = new(1, 1);

    private sealed record PendingPreview(Profile Profile, List<(IControlProvider Provider, Snapshot Snapshot)> Snapshots, CancellationTokenSource Timer);

    private PendingPreview? _preview;

    /// Raised (off the caller's thread) when a preview timed out and was
    /// reverted — the id of the previewed profile.
    public event Action<string>? PreviewReverted;

    public async Task<ApplyResult> ApplyProfileAsync(Profile profile, CancellationToken ct)
    {
        await _transactionLock.WaitAsync(ct);
        try
        {
            // A new apply supersedes a pending preview: undo it first, so
            // domains the new profile doesn't touch don't keep previewed
            // values nobody confirmed.
            RevertPreviewLocked();
            return Apply(profile).Result;
        }
        finally
        {
            _transactionLock.Release();
        }
    }

    /// Applies `profile` for `revertAfter`; see the class doc.
    public async Task<ApplyResult> PreviewAsync(Profile profile, TimeSpan revertAfter, CancellationToken ct)
    {
        await _transactionLock.WaitAsync(ct);
        try
        {
            RevertPreviewLocked();
            var (result, snapshots) = Apply(profile);
            if (!result.Ok)
                return result;

            var timer = new CancellationTokenSource();
            _preview = new PendingPreview(profile, snapshots, timer);
            _ = RevertWhenDueAsync(revertAfter, timer.Token);
            return result;
        }
        finally
        {
            _transactionLock.Release();
        }
    }

    /// Ends a pending preview: kept, or reverted now. Returns the previewed
    /// profile, or null when no preview was pending (already timed out).
    public async Task<Profile?> EndPreviewAsync(bool keep, CancellationToken ct)
    {
        await _transactionLock.WaitAsync(ct);
        try
        {
            if (_preview is not { } preview)
                return null;
            if (keep)
            {
                preview.Timer.Cancel();
                _preview = null;
                SidecarLog.Log($"[rigstats-control] Preview of '{preview.Profile.Id}' kept.");
            }
            else
            {
                RevertPreviewLocked();
            }
            return preview.Profile;
        }
        finally
        {
            _transactionLock.Release();
        }
    }

    private async Task RevertWhenDueAsync(TimeSpan delay, CancellationToken timer)
    {
        try
        {
            await Task.Delay(delay, timer);
        }
        catch (OperationCanceledException)
        {
            return; // kept or superseded.
        }
        string? reverted = null;
        await _transactionLock.WaitAsync(CancellationToken.None);
        try
        {
            if (_preview is { } preview && preview.Timer.Token == timer)
            {
                reverted = preview.Profile.Id;
                RevertPreviewLocked();
            }
        }
        finally
        {
            _transactionLock.Release();
        }
        if (reverted is not null)
            PreviewReverted?.Invoke(reverted);
    }

    // Caller holds _transactionLock.
    private void RevertPreviewLocked()
    {
        if (_preview is not { } preview)
            return;
        _preview = null;
        preview.Timer.Cancel();
        RestoreAll(preview.Snapshots);
        SidecarLog.Log($"[rigstats-control] Preview of '{preview.Profile.Id}' reverted.");
    }

    private (ApplyResult Result, List<(IControlProvider, Snapshot)> Snapshots) Apply(Profile profile)
    {
        var affected = DomainOrder
            .Where(HasPart(profile.Part))
            .Select(domain => _byDomain.TryGetValue(domain, out var p) ? p : null)
            .Where(p => p is not null)
            .Select(p => p!)
            .ToList();

        if (affected.Count == 0)
            return (ApplyResult.Success(profile.Id), []);

        // Every outcome lands in rigstats-sensor.log, which ships in the
        // diagnostics ZIP — the record of which boards/firmware accept which
        // writes (#207).
        var (result, snapshots) = Apply(profile, affected);
        var domains = string.Join(", ", affected.Select(p => p.Domain));
        SidecarLog.Log(result.Ok
            ? $"[rigstats-control] Applied profile '{profile.Id}' ({domains})."
            : $"[rigstats-control] Profile '{profile.Id}' not applied ({domains}), rolled back: {result.Message}");
        return (result, snapshots);
    }

    private static (ApplyResult, List<(IControlProvider, Snapshot)>) Apply(Profile profile, List<IControlProvider> affected)
    {
        // 1. Validate every affected domain up front — reject before touching
        //    anything if any single part is invalid.
        foreach (var provider in affected)
        {
            var validation = provider.Validate(profile.Part);
            if (!validation.Ok)
            {
                return (ApplyResult.Failure(
                    profile.Id,
                    $"Profile {profile.Name} not applied: {validation.Reason ?? $"{provider.Domain} rejected the requested value."}"), []);
            }
        }

        // 2. Snapshot every affected domain before changing anything, so a
        //    failure partway through can be fully rolled back.
        //    Capture reads hardware and can fail too (an SMU that doesn't
        //    answer) — nothing is written yet, so that's a plain failure.
        var snapshots = new List<(IControlProvider Provider, Snapshot Snapshot)>();
        foreach (var provider in affected)
        {
            try
            {
                snapshots.Add((provider, provider.Capture()));
            }
            catch (Exception e)
            {
                return (ApplyResult.Failure(profile.Id,
                    $"Profile {profile.Name} not applied: could not read the current {provider.Domain} state ({e.Message})."), []);
            }
        }

        // 3. Apply in the fixed order, verifying each as we go. On the first
        //    failure, restore everything touched, in reverse order — including
        //    the provider whose Apply threw: a multi-write provider (fans:
        //    one write per header) can fail after some writes landed, and
        //    restoring an untouched provider to its snapshot is harmless.
        var applied = new List<(IControlProvider, Snapshot)>();
        try
        {
            foreach (var (provider, snapshot) in snapshots)
            {
                applied.Add((provider, snapshot));
                provider.Apply(profile.Part);
                if (!provider.Verify(profile.Part))
                    throw new ControlApplyException($"{provider.Domain} did not take effect after applying.");
            }
        }
        catch (Exception e)
        {
            RestoreAll(applied);
            return (ApplyResult.Failure(profile.Id, $"Profile {profile.Name} not applied: {e.Message}"), []);
        }

        return (ApplyResult.Success(profile.Id), snapshots);
    }

    /// Restores in reverse order. Best-effort: a restore failure must not
    /// stop the remaining providers from being restored.
    private static void RestoreAll(List<(IControlProvider Provider, Snapshot Snapshot)> snapshots)
    {
        for (var i = snapshots.Count - 1; i >= 0; i--)
        {
            var (provider, snapshot) = snapshots[i];
            try
            {
                provider.Restore(snapshot);
            }
            catch (Exception restoreError)
            {
                SidecarLog.Log($"[rigstats-control] Rollback of {provider.Domain} failed: {restoreError.Message}");
            }
        }
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
