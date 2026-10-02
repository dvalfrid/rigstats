using System.IO.Pipes;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text.Json;
using System.Threading.Channels;
using Microsoft.Extensions.Hosting;
using SensorSidecar.Control.Providers;

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
    IPipeClientVerifier verifier,
    FanProvider fanProvider,
    FanCurveLoop fanLoop) : BackgroundService
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
        // Same as the telemetry pipe: SYSTEM as a service, the admin user
        // when run from an elevated console during development.
        security.AddAccessRule(new PipeAccessRule(
            WindowsIdentity.GetCurrent().User!,
            PipeAccessRights.FullControl,
            AccessControlType.Allow));
        return security;
    }

    private readonly PipeSecurity _pipeSecurity = BuildPipeSecurity();

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        SidecarLog.Log("[rigstats-control] Listening on \\\\.\\pipe\\rigstats-control");
        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                await AcceptOneAsync(stoppingToken);
            }
            catch (OperationCanceledException)
            {
                break;
            }
            catch (Exception e)
            {
                // A BackgroundService that throws out of ExecuteAsync stops
                // the whole host (the default BackgroundServiceExceptionBehavior
                // is StopHost) — without this catch, any exception here (e.g.
                // pipe/ACL construction failing) would take telemetry and fan
                // control down with it. Back off briefly so a persistent
                // failure doesn't spin the loop.
                SidecarLog.Log($"[rigstats-control] ExecuteAsync error: {e}");
                try
                {
                    await Task.Delay(TimeSpan.FromSeconds(2), stoppingToken);
                }
                catch (OperationCanceledException)
                {
                    break;
                }
            }
        }
    }

    private async Task AcceptOneAsync(CancellationToken stoppingToken)
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
            throw;
        }

        var verdict = verifier.Verify(pipe);
        if (!verdict.Allowed)
        {
            // Reject silently over the wire (no protocol response to an
            // unverified caller) but still log it — otherwise a legitimate
            // rigstats.exe being rejected (e.g. after a signer mismatch)
            // would be undiagnosable in the field.
            SidecarLog.Log($"[rigstats-control] Client rejected: {verdict.Reason}");
            await pipe.DisposeAsync();
            return;
        }

        SidecarLog.Log("[rigstats-control] Client connected.");
        // Control is deliberately single-client (doc: "one client at a
        // time") — serve fully before accepting the next connection, unlike
        // the telemetry pipe's fire-and-forget-per-client loop.
        await ServeClientAsync(pipe, stoppingToken);
        SidecarLog.Log("[rigstats-control] Client disconnected.");
    }

    /// One connected client: responses and pushed events share the writer,
    /// so every line goes out under `_writeLock`. Events are queued per
    /// connection (bounded, drop-oldest) and only once the client has sent
    /// `subscribe` — nothing from before the connection is ever replayed.
    private sealed class Connection(StreamWriter writer) : IDisposable
    {
        private readonly SemaphoreSlim _writeLock = new(1, 1);

        public Channel<ControlEventMessage> Events { get; } = Channel.CreateBounded<ControlEventMessage>(
            new BoundedChannelOptions(64) { FullMode = BoundedChannelFullMode.DropOldest });

        public bool Subscribed
        {
            get => Volatile.Read(ref _subscribed);
            set => Volatile.Write(ref _subscribed, value);
        }

        private bool _subscribed;

        public async Task WriteLineAsync<T>(T message, CancellationToken ct)
        {
            var json = JsonSerializer.Serialize(message, ControlJson.Options);
            await _writeLock.WaitAsync(ct);
            try
            {
                await writer.WriteLineAsync(json.AsMemory(), ct);
            }
            finally
            {
                _writeLock.Release();
            }
        }

        public void Dispose() => _writeLock.Dispose();
    }

    private async Task ServeClientAsync(NamedPipeServerStream pipe, CancellationToken stoppingToken)
    {
        using var connectionCts = CancellationTokenSource.CreateLinkedTokenSource(stoppingToken);
        var ct = connectionCts.Token;
        try
        {
            using var reader = new StreamReader(pipe);
            await using var writer = new StreamWriter(pipe) { AutoFlush = true };
            using var connection = new Connection(writer);

            void OnFanEvent(FanEvent e)
            {
                if (connection.Subscribed)
                    connection.Events.Writer.TryWrite(ToEventMessage(e));
            }

            fanLoop.EventRaised += OnFanEvent;
            var pump = PumpEventsAsync(connection, ct);
            try
            {
                while (pipe.IsConnected && !ct.IsCancellationRequested)
                {
                    var line = await reader.ReadLineAsync(ct);
                    if (line is null)
                        break; // client disconnected.
                    await HandleLineAsync(line, connection, ct);
                }
            }
            finally
            {
                // Stop the pump before the writer it uses is disposed.
                fanLoop.EventRaised -= OnFanEvent;
                connectionCts.Cancel();
                await pump;
            }
        }
        catch (IOException) { }
        catch (OperationCanceledException) { }
        finally
        {
            await pipe.DisposeAsync();
        }
    }

    private static async Task PumpEventsAsync(Connection connection, CancellationToken ct)
    {
        try
        {
            await foreach (var message in connection.Events.Reader.ReadAllAsync(ct))
                await connection.WriteLineAsync(message, ct);
        }
        catch (OperationCanceledException) { }
        catch (IOException) { } // client went away mid-write; the read loop notices too.
    }

    internal static ControlEventMessage ToEventMessage(FanEvent e) => e switch
    {
        FanEvent.SafetyTripped s => new ControlEventMessage { Event = "safety_tripped", Data = new { reason = s.Reason } },
        FanEvent.DutyUpdate d => new ControlEventMessage { Event = "fan_duty", Data = new { duty = d.DutyByHeader } },
        _ => throw new ArgumentOutOfRangeException(nameof(e)),
    };

    private async Task HandleLineAsync(string line, Connection connection, CancellationToken ct)
    {
        ControlRequest request;
        try
        {
            request = JsonSerializer.Deserialize<ControlRequest>(line, ControlJson.Options)
                ?? throw new JsonException("empty request.");
        }
        catch (JsonException e)
        {
            await connection.WriteLineAsync(ControlResponse.Fail(0, "bad_request", e.Message), ct);
            return;
        }

        var response = await DispatchAsync(request, connection, ct);
        await connection.WriteLineAsync(response, ct);
    }

    private async Task<ControlResponse> DispatchAsync(ControlRequest request, Connection connection, CancellationToken ct)
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
                "identify_fan" => HandleIdentifyFan(request),
                "subscribe" => HandleSubscribe(request, connection),
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

    private static ControlResponse HandleSubscribe(ControlRequest request, Connection connection)
    {
        connection.Subscribed = true;
        return ControlResponse.Ok(request.Id, new { subscribed = true });
    }

    /// Answers at once and runs the 3 s spin in the background — the control
    /// pipe serves one request at a time, so awaiting it would stall the UI.
    private ControlResponse HandleIdentifyFan(ControlRequest request)
    {
        var header = request.Params?.GetProperty("header").GetString()
            ?? throw new JsonException("missing header.");
        if (!fanProvider.HasHeader(header))
            return ControlResponse.Fail(request.Id, "not_found", $"fan header '{header}' not found.");

        _ = IdentifyInBackgroundAsync(header);
        return ControlResponse.Ok(request.Id, new { ok = true, duration_ms = (int)FanProvider.IdentifyDuration.TotalMilliseconds });
    }

    private async Task IdentifyInBackgroundAsync(string header)
    {
        try
        {
            await fanProvider.IdentifyAsync(header, FanProvider.IdentifyDuration, CancellationToken.None);
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] identify_fan {header} failed: {e}");
        }
    }
}
