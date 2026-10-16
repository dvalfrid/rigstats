using System.Collections.Concurrent;
using System.IO.Pipes;
using System.Security.AccessControl;
using System.Security.Principal;
using LibreHardwareMonitor.Hardware;
using Microsoft.Extensions.Hosting;
using SensorSidecar.Control;

namespace SensorSidecar;

// `pipeName` and `writeTimeout` are only overridden by tests: the pipe name
// must not collide with an installed service's pipe, and the timeout keeps
// them short.
public sealed class SensorWorker(
    IHardwareHost hardwareHost,
    string pipeName = "rigstats-sensors",
    TimeSpan? writeTimeout = null) : GuardedBackgroundService
{
    private readonly IHardwareHost _hardwareHost = hardwareHost;
    private readonly string _pipeName = pipeName;

    protected override string LogTag => "[rigstats-sensor]";

    // A sample that keeps failing drops every client, the app reconnects
    // every second, and each round would log three lines (#378): the failure
    // is logged once per reason, and connects/disconnects not while it lasts.
    private readonly RepeatLog _clientErrors = new("[rigstats-sensor] Telemetry failed, clients are dropped until it works again", "failed");

    /// A client that hasn't read a line within this is dropped (#232): the
    /// pipe is unbuffered, so its write would block, holding the instance
    /// forever. RIGStats reads every tick (~1 s) and closes the pipe when
    /// it pauses, so it never comes near this.
    private readonly TimeSpan _writeTimeout = writeTimeout ?? TimeSpan.FromSeconds(10);

    // Per-client tasks, awaited in StopAsync — base.StopAsync only awaits
    // ExecuteAsync, and a client still inside GetTelemetryLineAsync must be
    // done before HardwareHost closes the Computer (#204).
    private readonly ConcurrentDictionary<int, Task> _clients = new();

    // Allow BUILTIN\Users read access so user-mode RIGStats can connect when
    // this service runs as LocalSystem in session 0.
    private readonly PipeSecurity _pipeSecurity = BuildPipeSecurity();

    private static PipeSecurity BuildPipeSecurity()
    {
        var security = new PipeSecurity();
        security.AddAccessRule(new PipeAccessRule(
            new SecurityIdentifier(WellKnownSidType.BuiltinUsersSid, null),
            PipeAccessRights.Read | PipeAccessRights.Synchronize,
            AccessControlType.Allow));
        security.AddAccessRule(new PipeAccessRule(
            new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null),
            PipeAccessRights.FullControl,
            AccessControlType.Allow));
        // The creating account needs CreateNewInstance to offer the next
        // instance after the first client connects. As a service that is
        // SYSTEM (already covered); run from an elevated console for
        // development it is the admin user, who otherwise gets access denied.
        security.AddAccessRule(new PipeAccessRule(
            WindowsIdentity.GetCurrent().User!,
            PipeAccessRights.FullControl,
            AccessControlType.Allow));
        return security;
    }

    private static readonly string SensorTreePath =
        Path.Combine(DataDirectory.DefaultPath, "sensor-tree.txt");

    public override Task StartAsync(CancellationToken cancellationToken)
    {
        SidecarLog.TruncateIfNeeded();
        SidecarLog.Log($"[rigstats-sensor] Listening on \\\\.\\pipe\\{_pipeName}");
        _hardwareHost.WriteSensorTree(SensorTreePath);
        return base.StartAsync(cancellationToken);
    }

    // Several clients can be connected at once (e.g. the wallpaper host plus the
    // main app feeding the game overlay). LHM is not thread-safe and sampling is
    // the expensive part, so all clients share one cached sample via
    // `IHardwareHost.GetTelemetryLineAsync` (see `Control/HardwareHost.cs`):
    // whichever client finds it stale re-samples under the lock, the others
    // reuse it. RIGStats uses up to two; the rest leaves room for other
    // readers (a diagnostics script, a second user session) without leaving
    // the dashboard without data (#232).
    internal const int MaxClients = 8;

    /// One telemetry line per client per second.
    internal const int LineIntervalMs = 1000;

    protected override async Task RunAsync(CancellationToken stoppingToken)
    {
        // Log a run of busy-instance failures once, not every 2 s retry.
        var unavailableLogged = false;
        while (!stoppingToken.IsCancellationRequested)
        {
            NamedPipeServerStream pipe;
            try
            {
                pipe = NamedPipeServerStreamAcl.Create(
                    _pipeName,
                    PipeDirection.Out,
                    maxNumberOfServerInstances: MaxClients,
                    PipeTransmissionMode.Byte,
                    // With no client connected there is no instance of ours,
                    // so this one must create the pipe name itself — and
                    // fail if another process already holds it, rather than
                    // join that process's pipe.
                    _clients.IsEmpty ? PipeOptions.Asynchronous | PipeOptions.FirstPipeInstance : PipeOptions.Asynchronous,
                    inBufferSize: 0,
                    outBufferSize: 0,
                    pipeSecurity: _pipeSecurity);
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException)
            {
                // E.g. all MaxClients instances busy. Throwing out of
                // ExecuteAsync would stop the whole host (the default
                // BackgroundServiceExceptionBehavior is StopHost) — taking
                // fan control down with telemetry. Back off and retry.
                if (!unavailableLogged)
                    SidecarLog.Log($"[rigstats-sensor] Pipe instance unavailable: {e.Message}");
                unavailableLogged = true;
                try
                {
                    await Task.Delay(TimeSpan.FromSeconds(2), stoppingToken);
                }
                catch (OperationCanceledException)
                {
                    break;
                }
                continue;
            }
            unavailableLogged = false;

            try
            {
                await pipe.WaitForConnectionAsync(stoppingToken);
            }
            catch (OperationCanceledException)
            {
                await pipe.DisposeAsync();
                break;
            }
            catch (IOException)
            {
                // A client that connected and closed before this instance
                // accepted it ("The pipe is being closed", #378) — nothing to
                // serve; offer the next instance.
                await pipe.DisposeAsync();
                continue;
            }

            // Serve this client in the background and immediately offer the
            // next pipe instance to another client.
            var client = ServeClientAsync(pipe, stoppingToken);
            _clients[client.Id] = client;
            _ = client.ContinueWith(t => _clients.TryRemove(t.Id, out _), TaskScheduler.Default);
        }
    }

    private async Task ServeClientAsync(NamedPipeServerStream pipe, CancellationToken stoppingToken)
    {
        if (!_clientErrors.Active)
            SidecarLog.Log("[rigstats-sensor] Client connected.");
        try
        {
            using var writer = new StreamWriter(pipe) { AutoFlush = true };
            while (pipe.IsConnected && !stoppingToken.IsCancellationRequested)
            {
                // Cancellable: a client that stopped reading blocks the write
                // (unbuffered pipe) and must not hold up service shutdown.
                var sampleStarted = Environment.TickCount64;
                var line = await _hardwareHost.GetTelemetryLineAsync(stoppingToken);
                var sampleMs = Environment.TickCount64 - sampleStarted;
                using (var write = CancellationTokenSource.CreateLinkedTokenSource(stoppingToken))
                {
                    write.CancelAfter(_writeTimeout);
                    try
                    {
                        await writer.WriteLineAsync(line.AsMemory(), write.Token);
                    }
                    catch (OperationCanceledException) when (!stoppingToken.IsCancellationRequested)
                    {
                        SidecarLog.Log($"[rigstats-sensor] Client has not read for {_writeTimeout.TotalSeconds:0} s — dropping it.");
                        break;
                    }
                }
                if (_clientErrors.Reset())
                    SidecarLog.Log("[rigstats-sensor] Telemetry works again.");
                // A second between lines, less the time the sample took: the
                // app gives up on a read after 1.2 s, and the hardware being
                // busy (telemetry's 300 ms wait, a restarting worker) must not
                // push the next line past that (seen live, #374). The time the
                // write waited for the client still counts, so a client that
                // reads slower than a line a second sets the pace (#232).
                await Task.Delay(TimeSpan.FromMilliseconds(Math.Max(0, LineIntervalMs - sampleMs)), stoppingToken);
            }
        }
        catch (IOException) { }
        catch (OperationCanceledException) { }
        catch (Exception e)
        {
            if (_clientErrors.Line(e.Message, DateTimeOffset.Now) is { } line)
                SidecarLog.Log(line);
        }
        finally
        {
            await pipe.DisposeAsync();
        }
        if (!_clientErrors.Active)
            SidecarLog.Log("[rigstats-sensor] Client disconnected.");
    }

    public override async Task StopAsync(CancellationToken cancellationToken)
    {
        // Cancels stoppingToken, so every client leaves its loop, disposes
        // its pipe (the client sees EOF) and finishes.
        await base.StopAsync(cancellationToken);
        try
        {
            await Task.WhenAll(_clients.Values).WaitAsync(cancellationToken);
        }
        catch (OperationCanceledException)
        {
            // Host shutdown timeout — HardwareHost.StopAsync still closes
            // under its lock, so a straggler can't race the close.
        }
        SidecarLog.Log("[rigstats-sensor] Stopped.");
    }
}

// The visitor pattern is required by LHM — sensors are not updated automatically.
// VisitHardware must recurse into SubHardware for motherboard Super I/O sensors to update.
public sealed class UpdateVisitor : IVisitor
{
    public void VisitComputer(IComputer computer) => computer.Traverse(this);

    public void VisitHardware(IHardware hardware)
    {
        hardware.Update();
        foreach (var sub in hardware.SubHardware)
            sub.Accept(this);
    }

    public void VisitSensor(ISensor sensor) { }
    public void VisitParameter(IParameter parameter) { }
}
