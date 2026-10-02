using System.Collections.Concurrent;
using System.IO.Pipes;
using System.Security.AccessControl;
using System.Security.Principal;
using LibreHardwareMonitor.Hardware;
using Microsoft.Extensions.Hosting;
using SensorSidecar.Control;

namespace SensorSidecar;

// `pipeName` is only overridden by tests, which must not collide with an
// installed service's pipe.
public sealed class SensorWorker(IHardwareHost hardwareHost, string pipeName = "rigstats-sensors") : BackgroundService
{
    private readonly IHardwareHost _hardwareHost = hardwareHost;
    private readonly string _pipeName = pipeName;

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
        Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
            "se.codeby.rigstats",
            "sensor-tree.txt");

    public override Task StartAsync(CancellationToken cancellationToken)
    {
        SidecarLog.TruncateIfNeeded();
        SidecarLog.Log($"[rigstats-sensor] Hardware opened. Listening on \\\\.\\pipe\\{_pipeName}");
        _hardwareHost.WriteSensorTree(SensorTreePath);
        return base.StartAsync(cancellationToken);
    }

    // Several clients can be connected at once (e.g. the wallpaper host plus the
    // main app feeding the game overlay). LHM is not thread-safe and sampling is
    // the expensive part, so all clients share one cached sample via
    // `IHardwareHost.GetTelemetryLineAsync` (see `Control/HardwareHost.cs`):
    // whichever client finds it stale re-samples under the lock, the others
    // reuse it.
    private const int MaxClients = 4;

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
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
                    PipeOptions.Asynchronous,
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

            // Serve this client in the background and immediately offer the
            // next pipe instance to another client.
            var client = ServeClientAsync(pipe, stoppingToken);
            _clients[client.Id] = client;
            _ = client.ContinueWith(t => _clients.TryRemove(t.Id, out _), TaskScheduler.Default);
        }
    }

    private async Task ServeClientAsync(NamedPipeServerStream pipe, CancellationToken stoppingToken)
    {
        SidecarLog.Log("[rigstats-sensor] Client connected.");
        try
        {
            using var writer = new StreamWriter(pipe) { AutoFlush = true };
            while (pipe.IsConnected && !stoppingToken.IsCancellationRequested)
            {
                // Cancellable: a client that stopped reading blocks the write
                // (unbuffered pipe) and must not hold up service shutdown.
                var line = await _hardwareHost.GetTelemetryLineAsync(stoppingToken);
                await writer.WriteLineAsync(line.AsMemory(), stoppingToken);
                await Task.Delay(1000, stoppingToken);
            }
        }
        catch (IOException) { }
        catch (OperationCanceledException) { }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-sensor] Client error: {e.Message}");
        }
        finally
        {
            await pipe.DisposeAsync();
        }
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
