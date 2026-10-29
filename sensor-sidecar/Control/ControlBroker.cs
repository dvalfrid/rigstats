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
[System.Diagnostics.CodeAnalysis.SuppressMessage("Design", "CA1001", Justification = "Its CancellationTokenSources have no timer and hold no handle; one is replaced while its task may still wait on it, so it is left to the GC.")]
public sealed class ControlBroker(IEnumerable<IControlProvider> providers)
{
    // Fixed apply order from the design doc — later phases add the
    // providers for the domains that don't exist yet; anything with no
    // registered provider is simply skipped.
    // The mouse's and keyboard's onboard profiles come before the lighting:
    // each onboard profile carries its own saved lighting, so switching it
    // after Aura wrote the profile's colour put the old colours back a
    // moment later — a different one on each keyboard (seen live, #491).
    private static readonly string[] DomainOrder =
        ["power_plan", "cpu_limit", "curve_opt", "gpu", "fan", "mouse", "keyboard", "aura", "headset"];

    /// The parts a profile switch answers before (#495): the lighting and
    /// the wireless devices — slow (a sleeping mouse costs seconds) and
    /// harmless, so they follow right after the switch is answered, in the
    /// same order, and their outcome is pushed (`ApplyResultChanged`). The
    /// rest — power, CPU, GPU, fans — stays one transaction that the
    /// answer waits for.
    private static readonly HashSet<string> Afterwards = ["mouse", "keyboard", "aura", "headset"];

    private readonly Dictionary<string, IControlProvider> _byDomain =
        providers.ToDictionary(p => p.Domain);

    // Doc: "Transactions are serialised — there is never more than one in flight."
    private readonly SemaphoreSlim _transactionLock = new(1, 1);

    private sealed record PendingPreview(Profile Profile, List<(IControlProvider Provider, Snapshot Snapshot)> Snapshots, CancellationTokenSource Timer);

    private PendingPreview? _preview;

    /// Raised (off the caller's thread) when a preview timed out and was
    /// reverted — the id of the previewed profile.
    public event Action<string>? PreviewReverted;

    /// How often, and for how long, a part skipped because its device slept
    /// is tried again (#489). One read per sleeping device each time.
    public TimeSpan RetryEvery { get; init; } = TimeSpan.FromSeconds(5);
    public TimeSpan RetryFor { get; init; } = TimeSpan.FromMinutes(5);

    // The retry of the last apply's sleeping parts; a new apply ends it.
    private CancellationTokenSource? _retry;

    // The last switch's lighting and devices, applied after its answer.
    private CancellationTokenSource? _afterwards;

    /// The last switch's lighting and devices are done (tests, start-up).
    public Task Settled { get; private set; } = Task.CompletedTask;

    /// Raised (off the caller's thread) when a retried part caught up, or
    /// gave up — the apply's new result, for the app's footer (#489).
    public event Action<ApplyResult>? ApplyResultChanged;

    public async Task<ApplyResult> ApplyProfileAsync(Profile profile, CancellationToken ct)
    {
        await _transactionLock.WaitAsync(ct);
        try
        {
            // A new apply supersedes a pending preview: undo it first, so
            // domains the new profile doesn't touch don't keep previewed
            // values nobody confirmed.
            RevertPreviewLocked();
            _retry?.Cancel();
            _retry = null;
            _afterwards?.Cancel();
            _afterwards = null;
            var (result, _, _) = Apply(profile, d => !Afterwards.Contains(d));
            if (result.Ok && HasAny(profile, Afterwards))
            {
                _afterwards = new CancellationTokenSource();
                Settled = ApplyAfterwardsAsync(profile, result, _afterwards.Token);
            }
            return result;
        }
        finally
        {
            _transactionLock.Release();
        }
    }

    /// Writes `profile`'s lighting again — the active profile's, after the
    /// display came back on (#670): a keyboard or other device that lost
    /// power in sleep shows its own effect until then. Only the lighting;
    /// the onboard profiles are saved on the devices. A device that hasn't
    /// woken yet is tried again like after an apply (#489). Skipped while a
    /// preview is pending, so it doesn't overwrite the previewed lighting.
    public async Task<ApplyResult?> ReapplyLightingAsync(Profile profile, CancellationToken ct)
    {
        await _transactionLock.WaitAsync(ct);
        try
        {
            if (_preview is not null)
                return null;
            _retry?.Cancel();
            _retry = null;
            var (result, _, asleep) = Apply(profile, d => d == "aura");
            if (result.Ok && asleep.Count > 0)
            {
                _retry = new CancellationTokenSource();
                _ = RetryAsleepAsync(profile, asleep, _retry.Token);
            }
            return result;
        }
        finally
        {
            _transactionLock.Release();
        }
    }

    private bool HasAny(Profile profile, IReadOnlySet<string> domains) =>
        Affected(profile, domains.Contains).Count > 0;

    // The lighting and devices of a switch already answered (#495). Their
    // own transaction; a sleeping device is tried again (#489); anything to
    // say is pushed with the switch's own note in front.
    private async Task ApplyAfterwardsAsync(Profile profile, ApplyResult answered, CancellationToken ct)
    {
        try
        {
            await _transactionLock.WaitAsync(ct);
        }
        catch (OperationCanceledException)
        {
            return; // a newer switch took over before these started.
        }
        try
        {
            if (ct.IsCancellationRequested)
                return;
            var (result, _, asleep) = Apply(profile, Afterwards.Contains);
            if (result.Ok && asleep.Count > 0)
            {
                _retry = new CancellationTokenSource();
                _ = RetryAsleepAsync(profile, asleep, _retry.Token);
            }
            var note = result.Ok
                ? result.Message?.Replace($"Profile {profile.Name} applied; ", "", StringComparison.Ordinal)
                : $"its lighting and devices weren't set — {result.Message?.Replace($"Profile {profile.Name} not applied: ", "", StringComparison.Ordinal)}";
            // Pushed even with nothing to add: the app reads the devices'
            // pages again on it, now that they hold the profile's settings.
            var before = answered.Message is { } m ? m + " " : $"Profile {profile.Name} applied; ";
            ApplyResultChanged?.Invoke(note is null ? answered : ApplyResult.Success(profile.Id, before + note));
        }
        finally
        {
            _transactionLock.Release();
        }
    }

    // Tries the parts whose devices slept every RetryEvery until they all
    // answered or RetryFor passed; a newer apply cancels it.
    private async Task RetryAsleepAsync(Profile profile, List<IControlProvider> asleep, CancellationToken ct)
    {
        var deadline = DateTime.UtcNow + RetryFor;
        var still = new List<string>();
        try
        {
            while (asleep.Count > 0)
            {
                if (DateTime.UtcNow >= deadline)
                {
                    SidecarLog.Log($"[rigstats-control] Profile '{profile.Id}': {string.Join(", ", still)} still didn't answer after {RetryFor.TotalMinutes:0} minutes; kept as they are.");
                    ApplyResultChanged?.Invoke(ApplyResult.Success(profile.Id,
                        $"Profile {profile.Name} applied; {string.Join(" and ", still)} stayed asleep and kept {(still.Count == 1 ? "its" : "their")} own settings. Switch the profile again once {(still.Count == 1 ? "it is" : "they are")} awake."));
                    return;
                }
                await Task.Delay(RetryEvery, ct);
                await _transactionLock.WaitAsync(ct);
                try
                {
                    if (ct.IsCancellationRequested)
                        return;
                    still.Clear();
                    foreach (var provider in asleep.ToList())
                    {
                        try
                        {
                            if (provider is IRetryPart retry)
                                retry.Retry(profile.Part);
                            else
                                provider.Apply(profile.Part);
                            if (!provider.Verify(profile.Part))
                                throw new ControlApplyException($"{provider.Domain} did not take effect after applying.");
                            asleep.Remove(provider);
                            SidecarLog.Log($"[rigstats-control] Profile '{profile.Id}': {provider.Domain} applied now that it answered.");
                        }
                        catch (DeviceAsleepException e)
                        {
                            still.AddRange(e.Names);
                        }
                        catch (Exception e)
                        {
                            // Something else than sleep: stop trying that part.
                            asleep.Remove(provider);
                            SidecarLog.Log($"[rigstats-control] Profile '{profile.Id}': {provider.Domain} not applied on retry: {e.Message}");
                        }
                    }
                }
                finally
                {
                    _transactionLock.Release();
                }
            }
            ApplyResultChanged?.Invoke(ApplyResult.Success(profile.Id));
        }
        catch (OperationCanceledException)
        {
            // A newer apply took over.
        }
    }

    /// Applies `profile` for `revertAfter`; see the class doc.
    public async Task<ApplyResult> PreviewAsync(Profile profile, TimeSpan revertAfter, CancellationToken ct)
    {
        await _transactionLock.WaitAsync(ct);
        try
        {
            RevertPreviewLocked();
            // The preview writes every part itself: a retry of sleeping
            // devices, or the lighting still to follow a switch, would
            // otherwise write the earlier profile over it (#678).
            _retry?.Cancel();
            _retry = null;
            _afterwards?.Cancel();
            _afterwards = null;
            var (result, snapshots, _) = Apply(profile);
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

    private List<IControlProvider> Affected(Profile profile, Func<string, bool> include) => DomainOrder
        .Where(HasPart(profile.Part))
        .Where(include)
        .Select(domain => _byDomain.TryGetValue(domain, out var p) ? p : null)
        .Where(p => p is not null && (p is not IAffectsPart a || a.Affects(profile.Part)))
        .Select(p => p!)
        .ToList();

    private (ApplyResult Result, List<(IControlProvider, Snapshot)> Snapshots, List<IControlProvider> Asleep) Apply(
        Profile profile, Func<string, bool>? include = null)
    {
        var affected = Affected(profile, include ?? (_ => true));

        if (affected.Count == 0)
            return (ApplyResult.Success(profile.Id), [], []);

        // Every outcome lands in rigstats-sensor.log, which ships in the
        // diagnostics ZIP — the record of which boards/firmware accept which
        // writes (#207).
        var (result, snapshots, asleep) = Apply(profile, affected);
        var domains = string.Join(", ", affected.Select(p => p.Domain));
        SidecarLog.Log(result.Ok
            ? $"[rigstats-control] Applied profile '{profile.Id}' ({domains}){(result.Message is { } note ? $" — {note}" : ".")}"
            : $"[rigstats-control] Profile '{profile.Id}' not applied ({domains}), rolled back: {result.Message}");
        return (result, snapshots, asleep);
    }

    private static (ApplyResult, List<(IControlProvider, Snapshot)>, List<IControlProvider>) Apply(Profile profile, List<IControlProvider> affected)
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
                    $"Profile {profile.Name} not applied: {validation.Reason ?? $"{provider.Domain} rejected the requested value."}"), [], []);
            }
        }

        // 2. Snapshot every affected domain before changing anything, so a
        //    failure partway through can be fully rolled back.
        //    Capture reads hardware and can fail too (an SMU that doesn't
        //    answer) — nothing is written yet, so that's a plain failure.
        //    A domain that is down for a known reason (#370) is skipped: the
        //    rest of the profile still applies.
        var snapshots = new List<(IControlProvider Provider, Snapshot Snapshot)>();
        var skipped = new List<(string Domain, string Reason)>();
        var asleep = new List<IControlProvider>();
        foreach (var provider in affected)
        {
            try
            {
                snapshots.Add((provider, provider.Capture()));
            }
            catch (DomainUnavailableException e)
            {
                skipped.Add((provider.Domain, e.Message));
            }
            catch (Exception e)
            {
                return (ApplyResult.Failure(profile.Id,
                    $"Profile {profile.Name} not applied: could not read the current {provider.Domain} state ({e.Message})."), [], []);
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
                try
                {
                    provider.Apply(profile.Part);
                    if (!provider.Verify(profile.Part))
                        throw new ControlApplyException($"{provider.Domain} did not take effect after applying.");
                }
                catch (DomainUnavailableException e)
                {
                    // Went down while applying: nothing more can be written
                    // or restored there; the rest goes on. A sleeping device
                    // is tried again by the caller (#489).
                    applied.RemoveAt(applied.Count - 1);
                    skipped.Add((provider.Domain, e.Message));
                    if (e is DeviceAsleepException)
                        asleep.Add(provider);
                }
            }
        }
        catch (Exception e)
        {
            RestoreAll(applied);
            return (ApplyResult.Failure(profile.Id, $"Profile {profile.Name} not applied: {e.Message}"), [], []);
        }

        if (skipped.Count == 0)
            return (ApplyResult.Success(profile.Id), applied, []);
        // A sleeping device's own words say it catches up; other skips name
        // their domain.
        // The same sleeping keyboard can hold up two parts (its onboard
        // profile and its lighting): said once.
        var note = string.Join(" ", skipped.Select(s =>
            asleep.Any(a => a.Domain == s.Domain) ? s.Reason : $"{s.Domain} skipped: {s.Reason}").Distinct());
        // Nothing applied is a failure — unless every part only waits for a
        // sleeping device, which catches up when it wakes (#489, #495).
        if (applied.Count == 0 && asleep.Count < skipped.Count)
            return (ApplyResult.Failure(profile.Id, $"Profile {profile.Name} not applied: {note}"), [], []);
        return (ApplyResult.Success(profile.Id, $"Profile {profile.Name} applied; {note}"), applied, asleep);
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
        "mouse" => part.Mouse?.Profile is not null,
        "keyboard" => part.Keyboard?.Profile is not null,
        "headset" => part.Headset?.EqPreset is not null,
        _ => false,
    };
}

/// Raised internally when a provider's `Verify` fails right after `Apply` —
/// caught by `ControlBroker.Apply` to trigger rollback; never escapes it.
internal sealed class ControlApplyException(string message) : Exception(message);
