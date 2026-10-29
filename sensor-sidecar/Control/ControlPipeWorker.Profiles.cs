using System.Text.Json;

namespace SensorSidecar.Control;

public sealed partial class ControlPipeWorker
{
    private async Task<ControlResponse> HandleGetStateAsync(ControlRequest request, CancellationToken ct)
    {
        var activeId = await profiles.GetActiveIdAsync(ct);
        return ControlResponse.Ok(request.Id, new
        {
            active_profile = activeId,
            dry_run = safetyGuard.DryRun,
            crash_guard_notice = crashGuard.Notice,
            profiles_notice = profiles.Notice,
        });
    }

    private async Task<ControlResponse> HandleSaveProfileAsync(ControlRequest request, CancellationToken ct)
    {
        var profile = ParamsAs<Profile>(request, "profile");
        if (string.IsNullOrWhiteSpace(profile.Id) || profile.Id.Length > ProfileStore.MaxTextLength
            || profile.Name.Length > ProfileStore.MaxTextLength)
            throw new BadRequestException($"a profile id must be 1–{ProfileStore.MaxTextLength} characters and its name at most {ProfileStore.MaxTextLength}.");
        if (!await profiles.SaveProfileAsync(profile, ct))
            return ControlResponse.Fail(request.Id, "too_many_profiles", $"There are already {ProfileStore.MaxProfiles} profiles. Delete one to add another.");
        return ControlResponse.Ok(request.Id, new { ok = true });
    }

    private async Task<ControlResponse> HandleDeleteProfileAsync(ControlRequest request, CancellationToken ct)
    {
        var id = StringParam(request, "id");
        var wasActive = await profiles.GetActiveIdAsync(ct) == id;
        if (!await profiles.DeleteProfileAsync(id, ct))
            return ControlResponse.Fail(request.Id, "not_found", $"profile '{id}' not found.");
        // The store falls back to another profile as active; apply it, so the
        // hardware follows what the UI now shows as active.
        if (wasActive)
            await ApplyActiveAsync(ct);
        return ControlResponse.Ok(request.Id, new { ok = true });
    }

    /// `{"id": "gaming"}` — a built-in back to its defaults, re-applied when
    /// it is the active profile.
    private async Task<ControlResponse> HandleResetProfileAsync(ControlRequest request, CancellationToken ct)
    {
        var id = StringParam(request, "id");
        if (await profiles.ResetBuiltinAsync(id, ct) is null)
            return ControlResponse.Fail(request.Id, "not_builtin", $"profile '{id}' is not a built-in profile.");
        var result = await profiles.GetActiveIdAsync(ct) == id
            ? await ApplyActiveAsync(ct)
            : ApplyResult.Success(id);
        return ControlResponse.Ok(request.Id, result);
    }

    private async Task<ApplyResult> ApplyActiveAsync(CancellationToken ct)
    {
        var activeId = await profiles.GetActiveIdAsync(ct);
        if (activeId is null || await profiles.GetAsync(activeId, ct) is not { } active)
            return ApplyResult.Failure(activeId ?? "", "No active profile.");
        return await broker.ApplyProfileAsync(active, ct);
    }

    private async Task<ControlResponse> HandleApplyProfileAsync(ControlRequest request, CancellationToken ct)
    {
        var id = StringParam(request, "id");
        var profile = await profiles.GetAsync(id, ct);
        if (profile is null)
            return ControlResponse.Fail(request.Id, "not_found", $"profile '{id}' not found.");

        var result = await broker.ApplyProfileAsync(profile, ct);
        if (result.Ok)
        {
            await profiles.SetActiveAsync(id, ct);
            crashGuard.AcknowledgeNotice();
        }
        return ControlResponse.Ok(request.Id, result);
    }

    /// `{"profile": {...}, "seconds": 15}` — applies an unsaved (edited)
    /// profile; `confirm` stores it and makes it active, otherwise the broker
    /// reverts it when the time is up.
    private async Task<ControlResponse> HandlePreviewAsync(ControlRequest request, CancellationToken ct)
    {
        var profile = ObjectParam<Profile>(request, "profile");
        var duration = DefaultPreviewDuration;
        if (request.Params is { ValueKind: JsonValueKind.Object } p && p.TryGetProperty("seconds", out var seconds)
            && seconds.ValueKind == JsonValueKind.Number && seconds.TryGetInt32(out var s) && s > 0)
            duration = TimeSpan.FromSeconds(Math.Min(s, MaxPreviewDuration.TotalSeconds));

        var result = await broker.PreviewAsync(profile, duration, ct);
        return ControlResponse.Ok(request.Id, new
        {
            ok = result.Ok,
            message = result.Message,
            profile_id = result.ProfileId,
            revert_in_s = result.Ok ? (int?)duration.TotalSeconds : null,
        });
    }

    /// `{"keep": true}` (default) or `{"keep": false}` to revert now.
    private async Task<ControlResponse> HandleConfirmAsync(ControlRequest request, CancellationToken ct)
    {
        var keep = request.Params is not { ValueKind: JsonValueKind.Object } p || !p.TryGetProperty("keep", out var k)
            || k.ValueKind switch
            {
                JsonValueKind.True => true,
                JsonValueKind.False => false,
                _ => throw new BadRequestException("'keep' must be true or false."),
            };
        var profile = await broker.EndPreviewAsync(keep, ct);
        if (profile is null)
            return ControlResponse.Fail(request.Id, "no_preview", "The preview already ended and was reverted.");
        if (keep)
        {
            await profiles.SaveProfileAsync(profile, ct);
            await profiles.SetActiveAsync(profile.Id, ct);
            crashGuard.AcknowledgeNotice();
        }
        return ControlResponse.Ok(request.Id, new { ok = true, kept = keep, profile_id = profile.Id });
    }
}
