using System.IO.Pipes;
using System.Text.Json;
using LibreHardwareMonitor.Hardware;
using SensorSidecar;

var jsonOptions = new JsonSerializerOptions
{
    PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
};

var computer = new Computer
{
    IsCpuEnabled = true,
    IsGpuEnabled = true,
    IsMemoryEnabled = true,
    IsMotherboardEnabled = true,
    IsStorageEnabled = true,
};

computer.Open();
Console.Error.WriteLine("[rigstats-sensor] Hardware opened. Listening on \\\\.\\pipe\\rigstats-sensors");

var visitor = new UpdateVisitor();

while (true)
{
    using var pipe = new NamedPipeServerStream(
        "rigstats-sensors",
        PipeDirection.Out,
        maxNumberOfServerInstances: 1,
        transmissionMode: PipeTransmissionMode.Byte,
        options: PipeOptions.Asynchronous);

    await pipe.WaitForConnectionAsync();
    Console.Error.WriteLine("[rigstats-sensor] Client connected.");

    try
    {
        using var writer = new StreamWriter(pipe) { AutoFlush = true };
        while (pipe.IsConnected)
        {
            computer.Accept(visitor);
            var payload = SensorReader.Extract(computer);
            await writer.WriteLineAsync(JsonSerializer.Serialize(payload, jsonOptions));
            await Task.Delay(1000);
        }
    }
    catch (IOException) { }

    Console.Error.WriteLine("[rigstats-sensor] Client disconnected.");
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
