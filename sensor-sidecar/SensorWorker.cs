using System.IO.Pipes;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text.Json;
using LibreHardwareMonitor.Hardware;
using Microsoft.Extensions.Hosting;

namespace SensorSidecar;

public sealed class SensorWorker : BackgroundService
{
    private readonly Computer _computer = new()
    {
        IsCpuEnabled = true,
        IsGpuEnabled = true,
        IsMemoryEnabled = true,
        IsMotherboardEnabled = true,
        IsStorageEnabled = true,
    };

    private readonly UpdateVisitor _visitor = new();

    private readonly JsonSerializerOptions _jsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
    };

    // Allow BUILTIN\Users read access so user-mode RIGStats can connect when
    // this service runs as LocalSystem in session 0.
    private readonly PipeSecurity _pipeSecurity = BuildPipeSecurity();

    private static PipeSecurity BuildPipeSecurity()
    {
        var security = new PipeSecurity();
        security.AddAccessRule(new PipeAccessRule(
            new SecurityIdentifier(WellKnownSidType.BuiltinUsersSid, null),
            PipeAccessRights.ReadData | PipeAccessRights.Synchronize,
            AccessControlType.Allow));
        security.AddAccessRule(new PipeAccessRule(
            new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null),
            PipeAccessRights.FullControl,
            AccessControlType.Allow));
        return security;
    }

    public override Task StartAsync(CancellationToken cancellationToken)
    {
        _computer.Open();
        Console.Error.WriteLine("[rigstats-sensor] Hardware opened. Listening on \\\\.\\pipe\\rigstats-sensors");
        return base.StartAsync(cancellationToken);
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            var pipe = NamedPipeServerStreamAcl.Create(
                "rigstats-sensors",
                PipeDirection.Out,
                maxNumberOfServerInstances: 1,
                PipeTransmissionMode.Byte,
                PipeOptions.Asynchronous,
                inBufferSize: 0,
                outBufferSize: 0,
                pipeSecurity: _pipeSecurity);

            try
            {
                await pipe.WaitForConnectionAsync(stoppingToken);
                Console.Error.WriteLine("[rigstats-sensor] Client connected.");

                try
                {
                    using var writer = new StreamWriter(pipe) { AutoFlush = true };
                    while (pipe.IsConnected && !stoppingToken.IsCancellationRequested)
                    {
                        _computer.Accept(_visitor);
                        var payload = SensorReader.Extract(_computer);
                        await writer.WriteLineAsync(JsonSerializer.Serialize(payload, _jsonOptions));
                        await Task.Delay(1000, stoppingToken);
                    }
                }
                catch (IOException) { }

                Console.Error.WriteLine("[rigstats-sensor] Client disconnected.");
            }
            catch (OperationCanceledException) { }
            finally
            {
                await pipe.DisposeAsync();
            }
        }
    }

    public override Task StopAsync(CancellationToken cancellationToken)
    {
        _computer.Close();
        Console.Error.WriteLine("[rigstats-sensor] Stopped.");
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
