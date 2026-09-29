using System.IO.Pipes;
using System.Security.AccessControl;
using System.Security.Principal;
using LibreHardwareMonitor.Hardware;
using Microsoft.Extensions.Hosting;
using SensorSidecar.Control;

namespace SensorSidecar;

public sealed class SensorWorker(IHardwareHost hardwareHost) : BackgroundService
{
    private readonly IHardwareHost _hardwareHost = hardwareHost;

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
        SidecarLog.Log("[rigstats-sensor] Hardware opened. Listening on \\\\.\\pipe\\rigstats-sensors");
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
        while (!stoppingToken.IsCancellationRequested)
        {
            var pipe = NamedPipeServerStreamAcl.Create(
                "rigstats-sensors",
                PipeDirection.Out,
                maxNumberOfServerInstances: MaxClients,
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

            // Serve this client in the background and immediately offer the
            // next pipe instance to another client.
            _ = ServeClientAsync(pipe, stoppingToken);
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
                await writer.WriteLineAsync(await _hardwareHost.GetTelemetryLineAsync(stoppingToken));
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

    public override Task StopAsync(CancellationToken cancellationToken)
    {
        SidecarLog.Log("[rigstats-sensor] Stopped.");
        return base.StopAsync(cancellationToken);
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
