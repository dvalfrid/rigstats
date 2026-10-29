using System.IO.Pipes;
using System.Reflection;
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
public sealed partial class ControlPipeWorker(
    ControlBroker broker,
    ProfileStore profiles,
    SafetyGuard safetyGuard,
    IEnumerable<IControlProvider> providers,
    IPipeClientVerifier verifier,
    FanProvider fanProvider,
    FanCurveLoop fanLoop,
    BootCrashGuard crashGuard,
    Lighting.LightingProvider lighting,
    Lighting.HueLink hue,
    Mice.MouseProvider mice,
    // Only overridden by tests, which must not collide with an installed
    // service's pipe (same seam as SensorWorker).
    string pipeName = "rigstats-control",
    // A profile switched because of the keyboard (#459) reaches the app as
    // `profile_changed`.
    Keyboards.KeyboardFollower? keyboardFollower = null,
    // `sensor_tree`: the diagnostics export's tree as read now (#557).
    IHardwareHost? hardware = null) : GuardedBackgroundService
{
    /// The service's own version — the same as the app's (sensor-sidecar.csproj
    /// `<Version>`, bumped by release-please).
    private static readonly string AppVersion =
        typeof(ControlPipeWorker).Assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion
        ?? "0.0.0";
    private const int ProtocolVersion = 1;

    /// How long a `preview` lasts without `confirm` when the client doesn't say.
    internal static readonly TimeSpan DefaultPreviewDuration = TimeSpan.FromSeconds(15);
    private static readonly TimeSpan MaxPreviewDuration = TimeSpan.FromSeconds(60);

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

    /// The longest request line accepted — the app's own cap on a frame
    /// (`MAX_PIPE_LINE_BYTES`); a profile is a few KB.
    internal const int MaxLineBytes = 256 * 1024;

    protected override async Task RunAsync(CancellationToken stoppingToken)
    {
        SidecarLog.Log($"[rigstats-control] Listening on \\\\.\\pipe\\{pipeName}");
        try
        {
            await AcceptLoopAsync(stoppingToken);
        }
        finally
        {
            if (_listening is { } listening)
                await listening.DisposeAsync();
            _listening = null;
        }
    }

    private readonly RepeatLog _acceptFailures = new("[rigstats-control] Accepting a client failed", "failed");

    private async Task AcceptLoopAsync(CancellationToken stoppingToken)
    {
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
                // Handled here rather than by GuardedBackgroundService, so the
                // listening instance stays open and the pipe name stays ours.
                // Back off so a persistent failure doesn't spin the loop.
                if (_acceptFailures.Line(e.Message, DateTimeOffset.Now) is { } line)
                    SidecarLog.Log($"{line}{Environment.NewLine}{e}");
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

    // The instance waiting for the next client. One is open for as long as
    // the worker runs — the next is created before the connected client is
    // served — so the pipe name is never free for another process to take
    // and pose as the service. Clients are still served one at a time; a
    // second one waits on this instance until the first has left.
    private NamedPipeServerStream? _listening;
    private bool _nameTakenLogged;
    private readonly RepeatLog _refusals = new("Client rejected", "refused");

    /// `first`: no instance of ours exists, so creating one must also create
    /// the pipe name — it fails when another process already holds the name.
    private NamedPipeServerStream CreateInstance(bool first) =>
        NamedPipeServerStreamAcl.Create(
            pipeName,
            PipeDirection.InOut,
            maxNumberOfServerInstances: 2,
            PipeTransmissionMode.Byte,
            first ? PipeOptions.Asynchronous | PipeOptions.FirstPipeInstance : PipeOptions.Asynchronous,
            inBufferSize: 0,
            outBufferSize: 0,
            pipeSecurity: _pipeSecurity);

    private async Task AcceptOneAsync(CancellationToken stoppingToken)
    {
        NamedPipeServerStream pipe;
        try
        {
            pipe = _listening ?? CreateInstance(first: true);
            _listening = null;
            _nameTakenLogged = false;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            // Logged once per run of failures, not every retry.
            if (!_nameTakenLogged)
                SidecarLog.Log($"[rigstats-control] The pipe name is held by another process — the Control Center stays unavailable until it lets go: {e.Message}");
            _nameTakenLogged = true;
            await Task.Delay(TimeSpan.FromSeconds(2), stoppingToken);
            return;
        }

        try
        {
            await pipe.WaitForConnectionAsync(stoppingToken);
        }
        catch (IOException)
        {
            // A client that connected and closed before it was accepted
            // ("The pipe is being closed", #378): nothing to serve.
            await pipe.DisposeAsync();
            return;
        }
        catch
        {
            await pipe.DisposeAsync();
            throw;
        }
        _acceptFailures.Reset();

        try
        {
            _listening = CreateInstance(first: false);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            // The next round creates the name anew once this client has left.
            SidecarLog.Log($"[rigstats-control] Next pipe instance not created: {e.Message}");
        }

        var verdict = verifier.Verify(pipe);
        if (!verdict.Allowed)
        {
            // Reject silently over the wire (no protocol response to an
            // unverified caller) but still log it — otherwise a legitimate
            // rigstats.exe being rejected (e.g. after a signer mismatch)
            // would be undiagnosable in the field. Throttled: a refused app
            // keeps retrying (#231).
            var line = _refusals.Line(verdict.Reason ?? "", DateTimeOffset.Now);
            if (line is not null)
                SidecarLog.Log($"[rigstats-control] {line}");
            await pipe.DisposeAsync();
            return;
        }

        _refusals.Reset();
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
            var reader = new BoundedLineReader(pipe, MaxLineBytes);
            await using var writer = new StreamWriter(pipe) { AutoFlush = true };
            using var connection = new Connection(writer);

            void OnFanEvent(FanEvent e)
            {
                if (connection.Subscribed)
                    connection.Events.Writer.TryWrite(ToEventMessage(e));
            }

            void OnPreviewReverted(string id)
            {
                if (connection.Subscribed)
                    connection.Events.Writer.TryWrite(PreviewRevertedMessage(id));
            }

            void OnApplyResultChanged(ApplyResult result)
            {
                if (connection.Subscribed)
                    connection.Events.Writer.TryWrite(ApplyResultMessage(result));
            }

            void OnProfileSwitched(string id)
            {
                if (connection.Subscribed)
                    connection.Events.Writer.TryWrite(ProfileChangedMessage(id));
            }

            fanLoop.EventRaised += OnFanEvent;
            broker.PreviewReverted += OnPreviewReverted;
            broker.ApplyResultChanged += OnApplyResultChanged;
            if (keyboardFollower is not null)
                keyboardFollower.ProfileSwitched += OnProfileSwitched;
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
                broker.PreviewReverted -= OnPreviewReverted;
                broker.ApplyResultChanged -= OnApplyResultChanged;
                if (keyboardFollower is not null)
                    keyboardFollower.ProfileSwitched -= OnProfileSwitched;
                connectionCts.Cancel();
                await pump;
            }
        }
        catch (LineTooLongException e)
        {
            // Not a request the app sends: no answer, the connection ends.
            SidecarLog.Log($"[rigstats-control] Client sent {e.Message} — disconnecting it.");
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

    // The events, built here for the pipe and for the shared contract
    // example the app's reader checks too (#679).
    internal static ControlEventMessage PreviewRevertedMessage(string id) => new() { Event = "preview_reverted", Data = new { id } };
    internal static ControlEventMessage ApplyResultMessage(ApplyResult result) => new() { Event = "apply_result", Data = result };
    internal static ControlEventMessage ProfileChangedMessage(string id) => new() { Event = "profile_changed", Data = new { id } };

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
                "hello" => HandleHello(request),
                "capabilities" => ControlResponse.Ok(request.Id, providers.Select(SafeProbe).ToList()),
                "get_state" => await HandleGetStateAsync(request, ct),
                "list_profiles" => ControlResponse.Ok(request.Id, await profiles.ListAsync(ct)),
                "save_profile" => await HandleSaveProfileAsync(request, ct),
                "delete_profile" => await HandleDeleteProfileAsync(request, ct),
                "reset_profile" => await HandleResetProfileAsync(request, ct),
                "aura_preview" => HandleAuraPreview(request),
                "lamp_toggle" => HandleLampToggle(request),
                "aura_enable_wdl" => HandleAuraEnableWdl(request),
                "headset_settings" => ControlResponse.Ok(request.Id, HeadsetsReply(lighting)),
                "headset_set" => HandleHeadsetSet(request),
                "headset_eq_set" => HandleHeadsetEqSet(request),
                "keyboard_settings" => ControlResponse.Ok(request.Id, KeyboardsReply(lighting)),
                "keyboard_set" => HandleKeyboardSet(request),
                "mouse_settings" => ControlResponse.Ok(request.Id, MiceReply(mice)),
                "mouse_set" => HandleMouseSet(request),
                "hue_discover" => await HandleHueDiscoverAsync(request, ct),
                "hue_pair" => await HandleHuePairAsync(request, ct),
                "hue_refresh" => await HandleHueRefreshAsync(request, ct),
                "hue_choose" => HandleHueChoose(request),
                "hue_unpair" => HandleHueUnpair(request),
                "apply_profile" => await HandleApplyProfileAsync(request, ct),
                "preview" => await HandlePreviewAsync(request, ct),
                "confirm" => await HandleConfirmAsync(request, ct),
                "release_to_firmware" => HandleReleaseToFirmware(request),
                "identify_fan" => HandleIdentifyFan(request, connection),
                "subscribe" => HandleSubscribe(request, connection),
                "sensor_tree" => HandleSensorTree(request),
                _ => ControlResponse.Fail(request.Id, "unknown_method", $"unknown method '{request.Method}'."),
            };
        }
        catch (BadRequestException e)
        {
            // The request itself is wrong (a parameter missing or of the wrong
            // kind): said to the client, logged without a stack (#378).
            SidecarLog.Log($"[rigstats-control] '{Printable(request.Method)}' refused: {e.Message}");
            return ControlResponse.Fail(request.Id, "bad_request", e.Message);
        }
        catch (Exception e)
        {
            // In the service log too, so a diagnostics export shows it — with the
            // stack when it isn't one of our own explained failures.
            var detail = e is InvalidOperationException or JsonException or Lighting.HueUnreachableException ? e.Message : e.ToString();
            SidecarLog.Log($"[rigstats-control] '{Printable(request.Method)}' failed: {detail}");
            return ControlResponse.Fail(request.Id, "internal_error", e.Message);
        }
    }

    /// A client's string as it may go into the log: no line breaks or other
    /// control characters (which would forge log lines in a diagnostics
    /// export), at most 64 characters.
    internal static string Printable(string text)
    {
        const int max = 64;
        var clean = new string(text.Where(c => !char.IsControl(c)).ToArray());
        return clean.Length > max ? clean[..max] + "…" : clean;
    }

    // ── Parameters ──────────────────────────────────────────────────────
    // A parameter that is missing or of the wrong kind is a `bad_request`,
    // not an `internal_error` with a stack trace (#378).

    private static JsonElement Param(ControlRequest request, string name) =>
        request.Params is { ValueKind: JsonValueKind.Object } p && p.TryGetProperty(name, out var value)
            && value.ValueKind != JsonValueKind.Null
            ? value
            : throw new BadRequestException($"missing '{name}'.");

    private static string StringParam(ControlRequest request, string name) =>
        Param(request, name) is { ValueKind: JsonValueKind.String } value
            ? value.GetString()!
            : throw new BadRequestException($"'{name}' must be a string.");

    private static T ObjectParam<T>(JsonElement value, string name)
    {
        try
        {
            return value.Deserialize<T>(ControlJson.Options) ?? throw new BadRequestException($"missing '{name}'.");
        }
        catch (JsonException e)
        {
            throw new BadRequestException($"'{name}' is not valid: {e.Message}");
        }
    }

    private static T ObjectParam<T>(ControlRequest request, string name) => ObjectParam<T>(Param(request, name), name);

    /// The whole `params` object as `T` (save_profile sends the profile itself).
    private static T ParamsAs<T>(ControlRequest request, string what) =>
        request.Params is { ValueKind: JsonValueKind.Object } p
            ? ObjectParam<T>(p, what)
            : throw new BadRequestException($"missing {what}.");

    /// One domain's capability; a provider that throws is reported as
    /// unavailable instead of failing the whole list (and with it every tab
    /// of the Control Center).
    private static CapabilitySet SafeProbe(IControlProvider provider)
    {
        try
        {
            return provider.Probe();
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] {provider.Domain}: capability probe failed: {(e is Workers.HardwareWorkerDownException ? e.Message : e.ToString())}");
            return new CapabilitySet
            {
                Domain = provider.Domain,
                Supported = false,
                Reason = $"Unavailable: the {provider.Domain} hardware did not answer.",
                Problem = ControlProblems.DidNotAnswer(provider.Domain, e.Message),
            };
        }
    }

    /// `{"protocol": 1, "app_version": "1.53.0"}` — answered with the
    /// service's own; both are logged, so an app and service that don't match
    /// show in a diagnostics ZIP (#679). The app decides what a mismatch
    /// means.
    private static ControlResponse HandleHello(ControlRequest request)
    {
        string? app = null;
        int? protocol = null;
        if (request.Params is { ValueKind: JsonValueKind.Object } p)
        {
            if (p.TryGetProperty("app_version", out var v) && v.ValueKind == JsonValueKind.String)
                app = Printable(v.GetString()!);
            if (p.TryGetProperty("protocol", out var n) && n.ValueKind == JsonValueKind.Number && n.TryGetInt32(out var number))
                protocol = number;
        }
        SidecarLog.Log($"[rigstats-control] Hello from app {app ?? "?"} (protocol {protocol?.ToString(System.Globalization.CultureInfo.InvariantCulture) ?? "?"}); service {AppVersion} (protocol {ProtocolVersion}).");
        return ControlResponse.Ok(request.Id, new { protocol = ProtocolVersion, service_version = AppVersion });
    }

    /// The sensor tree as read now (#557) — `sensor-tree.txt` is only written
    /// at start, so it can't show a sensor that went missing later.
    private ControlResponse HandleSensorTree(ControlRequest request) =>
        hardware is null
            ? ControlResponse.Fail(request.Id, "unavailable", "The sensors aren't read by this service.")
            : ControlResponse.Ok(request.Id, new { tree = hardware.SensorTree() });

    private static ControlResponse HandleSubscribe(ControlRequest request, Connection connection)
    {
        connection.Subscribed = true;
        return ControlResponse.Ok(request.Id, new { subscribed = true });
    }
}
