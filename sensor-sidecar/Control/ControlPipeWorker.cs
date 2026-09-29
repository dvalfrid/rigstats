using System.IO.Pipes;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text.Json;
using Microsoft.Extensions.Hosting;

namespace SensorSidecar.Control;

/// The control pipe (`\\.\pipe\rigstats-control`) — duplex, one client at a
/// time, newline-delimited JSON, request/response correlated by `id` plus
/// pushed `event` lines. See `docs/control-architecture.md`, "Control pipe
/// protocol". Every connection is verified by `IPipeClientVerifier` before
/// any request is processed.
public sealed class ControlPipeWorker(
    ControlBroker broker,
    ProfileStore profiles,
    SafetyGuard safetyGuard,
    IEnumerable<IControlProvider> providers,
    IPipeClientVerifier verifier) : BackgroundService
{
    private const string AppVersion = "3.0.0"; // TODO: pull from the assembly/installer version once wired up.
    private const int ProtocolVersion = 1;

    private static PipeSecurity BuildPipeSecurity()
    {
        var security = new PipeSecurity();
        // Interactive user read/write (not just read, unlike telemetry) —
        // this is a write path, but still restricted to SYSTEM + the
        // logged-in interactive user; `PipeClientVerifier` does the real
        // authorization on top of this.
        security.AddAccessRule(new PipeAccessRule(
            new SecurityIdentifier(WellKnownSidType.InteractiveSid, null),
            PipeAccessRights.ReadWrite | PipeAccessRights.Synchronize,
            AccessControlType.Allow));
        security.AddAccessRule(new PipeAccessRule(
            new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null),
            PipeAccessRights.FullControl,
            AccessControlType.Allow));
        return security;
    }

    private readonly PipeSecurity _pipeSecurity = BuildPipeSecurity();

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            var pipe = NamedPipeServerStreamAcl.Create(
                "rigstats-control",
                PipeDirection.InOut,
                maxNumberOfServerInstances: 1,
                PipeTransmissionMode.Byte,
                PipeOptions.Asynchronous,
                inBufferSize: 0,
                outBufferSize: 0,
                pipeSecurity: _pipeSecurity);

            try
            {
                await pipe.WaitForConnectionAsync(stoppingToken);
            }
            catch (OperationCanceledException)
            {
                await pipe.DisposeAsync();
                break;
            }

            var verdict = verifier.Verify(pipe);
            if (!verdict.Allowed)
            {
                // Reject silently over the wire (no protocol response to an
                // unverified caller) but still log it — otherwise a
                // legitimate rigstats.exe being rejected (e.g. after a
                // signer mismatch) would be undiagnosable in the field.
                SidecarLog.Log($"[rigstats-control] Client rejected: {verdict.Reason}");
                await pipe.DisposeAsync();
                continue;
            }

            SidecarLog.Log("[rigstats-control] Client connected.");
            // Control is deliberately single-client (doc: "one client at a
            // time") — serve fully before accepting the next connection,
            // unlike the telemetry pipe's fire-and-forget-per-client loop.
            await ServeClientAsync(pipe, stoppingToken);
            SidecarLog.Log("[rigstats-control] Client disconnected.");
        }
    }

    private async Task ServeClientAsync(NamedPipeServerStream pipe, CancellationToken stoppingToken)
    {
        try
        {
            using var reader = new StreamReader(pipe);
            await using var writer = new StreamWriter(pipe) { AutoFlush = true };
            while (pipe.IsConnected && !stoppingToken.IsCancellationRequested)
            {
                var line = await reader.ReadLineAsync(stoppingToken);
                if (line is null)
                    break; // client disconnected.
                await HandleLineAsync(line, writer, stoppingToken);
            }
        }
        catch (IOException) { }
        catch (OperationCanceledException) { }
        finally
        {
            await pipe.DisposeAsync();
        }
    }

    private async Task HandleLineAsync(string line, StreamWriter writer, CancellationToken ct)
    {
        ControlRequest request;
        try
        {
            request = JsonSerializer.Deserialize<ControlRequest>(line, ControlJson.Options)
                ?? throw new JsonException("empty request.");
        }
        catch (JsonException e)
        {
            await WriteAsync(writer, ControlResponse.Fail(0, "bad_request", e.Message), ct);
            return;
        }

        var response = await DispatchAsync(request, ct);
        await WriteAsync(writer, response, ct);
    }

    private async Task<ControlResponse> DispatchAsync(ControlRequest request, CancellationToken ct)
    {
        try
        {
            return request.Method switch
            {
                "hello" => ControlResponse.Ok(request.Id, new { protocol = ProtocolVersion, service_version = AppVersion }),
                "capabilities" => ControlResponse.Ok(request.Id, providers.Select(p => p.Probe()).ToList()),
                "get_state" => await HandleGetStateAsync(request, ct),
                "list_profiles" => ControlResponse.Ok(request.Id, await profiles.ListAsync(ct)),
                "save_profile" => await HandleSaveProfileAsync(request, ct),
                "delete_profile" => await HandleDeleteProfileAsync(request, ct),
                "apply_profile" => await HandleApplyProfileAsync(request, ct),
                "preview" => await HandleApplyProfileAsync(request, ct), // TODO(#187 follow-up): auto-revert timer; applies directly for now.
                "confirm" => ControlResponse.Ok(request.Id, new { ok = true }),
                "release_to_firmware" => HandleReleaseToFirmware(request),
                "identify_fan" => ControlResponse.Fail(request.Id, "not_supported", "no fan provider registered yet."),
                "subscribe" => ControlResponse.Ok(request.Id, new { subscribed = true }),
                _ => ControlResponse.Fail(request.Id, "unknown_method", $"unknown method '{request.Method}'."),
            };
        }
        catch (Exception e)
        {
            return ControlResponse.Fail(request.Id, "internal_error", e.Message);
        }
    }

    private async Task<ControlResponse> HandleGetStateAsync(ControlRequest request, CancellationToken ct)
    {
        var activeId = await profiles.GetActiveIdAsync(ct);
        return ControlResponse.Ok(request.Id, new { active_profile = activeId, dry_run = safetyGuard.DryRun });
    }

    private async Task<ControlResponse> HandleSaveProfileAsync(ControlRequest request, CancellationToken ct)
    {
        var profile = request.Params?.Deserialize<Profile>(ControlJson.Options)
            ?? throw new JsonException("missing profile.");
        await profiles.SaveProfileAsync(profile, ct);
        return ControlResponse.Ok(request.Id, new { ok = true });
    }

    private async Task<ControlResponse> HandleDeleteProfileAsync(ControlRequest request, CancellationToken ct)
    {
        var id = request.Params?.GetProperty("id").GetString()
            ?? throw new JsonException("missing id.");
        var deleted = await profiles.DeleteProfileAsync(id, ct);
        return deleted
            ? ControlResponse.Ok(request.Id, new { ok = true })
            : ControlResponse.Fail(request.Id, "not_found", $"profile '{id}' not found.");
    }

    private async Task<ControlResponse> HandleApplyProfileAsync(ControlRequest request, CancellationToken ct)
    {
        var id = request.Params?.GetProperty("id").GetString()
            ?? throw new JsonException("missing id.");
        var profile = await profiles.GetAsync(id, ct);
        if (profile is null)
            return ControlResponse.Fail(request.Id, "not_found", $"profile '{id}' not found.");

        var result = await broker.ApplyProfileAsync(profile, ct);
        if (result.Ok)
            await profiles.SetActiveAsync(id, ct);
        return ControlResponse.Ok(request.Id, result);
    }

    private ControlResponse HandleReleaseToFirmware(ControlRequest request)
    {
        safetyGuard.ReleaseAllToFirmware();
        return ControlResponse.Ok(request.Id, new { ok = true });
    }

    private static async Task WriteAsync(StreamWriter writer, ControlResponse response, CancellationToken ct)
    {
        var json = JsonSerializer.Serialize(response, ControlJson.Options);
        await writer.WriteLineAsync(json.AsMemory(), ct);
    }
}
