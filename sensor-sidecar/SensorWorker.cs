using System.IO.Pipes;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text.Json;
using LibreHardwareMonitor.Hardware;
using Microsoft.Extensions.Hosting;

namespace SensorSidecar;

public sealed class SensorWorker : BackgroundService
{
    private static readonly string LogPath =
        Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
            "se.codeby.rigstats",
            "rigstats-sensor.log");

    private static void Log(string message)
    {
        var line = $"[{DateTimeOffset.UtcNow:yyyy-MM-dd HH:mm:ss}Z] {message}";
        Console.Error.WriteLine(line);
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(LogPath)!);
            File.AppendAllText(LogPath, line + Environment.NewLine);
        }
        catch { }
    }

    // Keep the log from growing indefinitely: when it exceeds 512 KB, truncate
    // to the last 500 lines so recent context is always preserved.
    private static void TruncateLogIfNeeded()
    {
        try
        {
            if (File.Exists(LogPath) && new FileInfo(LogPath).Length > 512 * 1024)
            {
                var lines = File.ReadAllLines(LogPath);
                File.WriteAllLines(LogPath, lines.TakeLast(500));
            }
        }
        catch { }
    }

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
        TruncateLogIfNeeded();
        _computer.Open();
        Log("[rigstats-sensor] Hardware opened. Listening on \\\\.\\pipe\\rigstats-sensors");
        WriteSensorTree();
        return base.StartAsync(cancellationToken);
    }

    private void WriteSensorTree()
    {
        try
        {
            _computer.Accept(_visitor);
            var lines = new System.Text.StringBuilder();
            lines.AppendLine($"# sensor-tree — {DateTimeOffset.UtcNow:yyyy-MM-dd HH:mm:ss}Z");
            foreach (var hw in _computer.Hardware)
            {
                lines.AppendLine($"HW  {hw.HardwareType,-20} id={hw.Identifier} name={hw.Name}");
                foreach (var s in hw.Sensors)
                    lines.AppendLine($"  S  {s.SensorType,-15} id={s.Identifier} name={s.Name} val={s.Value}");
                foreach (var sub in hw.SubHardware)
                {
                    lines.AppendLine($"  SUB {sub.HardwareType,-18} id={sub.Identifier} name={sub.Name}");
                    foreach (var s in sub.Sensors)
                        lines.AppendLine($"    S  {s.SensorType,-15} id={s.Identifier} name={s.Name} val={s.Value}");
                }
            }
            Directory.CreateDirectory(Path.GetDirectoryName(SensorTreePath)!);
            File.WriteAllText(SensorTreePath, lines.ToString());
        }
        catch { }
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
                Log("[rigstats-sensor] Client connected.");

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

                Log("[rigstats-sensor] Client disconnected.");
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
        Log("[rigstats-sensor] Stopped.");
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
