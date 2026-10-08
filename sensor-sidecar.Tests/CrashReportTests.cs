using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Earlier crashes from the Windows event log into the service log (#242).
/// The messages are the real ones Windows wrote for the G14's NVML crash.
/// </summary>
public sealed class CrashReportTests
{
    private static readonly DateTimeOffset CrashTime = new(2026, 10, 6, 23, 6, 33, TimeSpan.FromHours(2));

    private const string DotNetRuntime = """
        Application: rigstats-sensor.exe
        CoreCLR Version: 10.0.1226.42308
        .NET Version: 10.0.12
        Description: The process was terminated due to an unhandled exception.
        Stack:
           at LibreHardwareMonitor.Interop.NvidiaML.NvmlDeviceGetPowerUsage(NvmlDevice)
           at LibreHardwareMonitor.Hardware.Gpu.NvidiaGpu.Update()
           at SensorSidecar.UpdateVisitor.VisitHardware(LibreHardwareMonitor.Hardware.IHardware)
           at LibreHardwareMonitor.Hardware.Computer.Traverse(LibreHardwareMonitor.Hardware.IVisitor)
           at SensorSidecar.UpdateVisitor.VisitComputer(LibreHardwareMonitor.Hardware.IComputer)
           at SensorSidecar.Control.HardwareHost.SampleLhm()
           at SensorSidecar.Control.HardwareHost.RefreshIfStale()
           at SensorSidecar.Control.HardwareHost+<GetTelemetryLineAsync>d__17.MoveNext()
           at System.Threading.ExecutionContext.RunInternal(System.Threading.ExecutionContext, System.Threading.ContextCallback, System.Object)
           at System.Threading.Tasks.Task.RunContinuations(System.Object)
           at System.Threading.Tasks.Task+DelayPromise.CompleteTimedOut()
           at System.Threading.TimerQueue.FireNextTimers()
           at System.Threading.ThreadPoolWorkQueue.Dispatch()
           at System.Threading.Thread.StartCallback()
        """;

    private const string ApplicationError = """
        Faulting application name: rigstats-sensor.exe, version: 1.43.0.0, time stamp: 0x6a890000
        Faulting module name: coreclr.dll, version: 10.0.1226.42308, time stamp: 0x6a89b724
        Exception code: 0xc0000005
        Fault offset: 0x00000000003596cf
        Faulting process id: 0x1A80
        Faulting application path: C:\Program Files\RIGStats\rigstats-sensor.exe
        """;

    [Fact]
    public void A_runtime_crash_is_logged_with_its_cause_and_stack()
    {
        var lines = CrashReport.Lines([new CrashEvent(CrashTime, 1026, DotNetRuntime)], CrashTime.AddDays(-1));

        var line = Assert.Single(lines);
        Assert.Contains("An earlier run crashed at 2026-10-06", line);
        Assert.Contains("The process was terminated due to an unhandled exception.", line);
        Assert.Contains("at LibreHardwareMonitor.Interop.NvidiaML.NvmlDeviceGetPowerUsage(NvmlDevice)", line);
    }

    [Fact]
    public void A_long_stack_is_cut_and_says_how_much_is_left_out()
    {
        var line = Assert.Single(CrashReport.Lines([new CrashEvent(CrashTime, 1026, DotNetRuntime)], DateTimeOffset.MinValue));

        Assert.Contains("… 2 more frames", line);
        Assert.DoesNotContain("Thread.StartCallback", line);
    }

    [Fact]
    public void A_native_fault_is_logged_with_module_code_and_offset()
    {
        var line = Assert.Single(CrashReport.Lines([new CrashEvent(CrashTime, 1000, ApplicationError)], DateTimeOffset.MinValue));

        Assert.Contains("exception 0xc0000005 in coreclr.dll at offset 0x00000000003596cf", line);
    }

    [Fact]
    public void Other_programs_and_crashes_already_logged_are_left_out()
    {
        var events = new[]
        {
            new CrashEvent(CrashTime, 1026, DotNetRuntime.Replace("rigstats-sensor.exe", "SomeOtherApp.exe")),
            new CrashEvent(CrashTime.AddDays(-2), 1026, DotNetRuntime), // before the mark
            new CrashEvent(CrashTime, 1000, ApplicationError.Replace("rigstats-sensor.exe", "rigstats.exe")),
        };

        Assert.Empty(CrashReport.Lines(events, CrashTime.AddDays(-1)));
    }

    [Fact]
    public void A_native_fault_with_a_runtime_entry_for_the_same_crash_is_logged_once()
    {
        var events = new[]
        {
            new CrashEvent(CrashTime, 1026, DotNetRuntime),
            new CrashEvent(CrashTime.AddMilliseconds(400), 1000, ApplicationError),
        };

        var line = Assert.Single(CrashReport.Lines(events, DateTimeOffset.MinValue));
        Assert.Contains("NvmlDeviceGetPowerUsage", line);
    }

    [Fact]
    public void Crashes_are_logged_oldest_first()
    {
        var events = new[]
        {
            new CrashEvent(CrashTime, 1000, ApplicationError),
            new CrashEvent(CrashTime.AddDays(-1), 1026, DotNetRuntime),
        };

        var lines = CrashReport.Lines(events, DateTimeOffset.MinValue);

        Assert.Equal(2, lines.Count);
        Assert.Contains("unhandled exception", lines[0]);
        Assert.Contains("0xc0000005", lines[1]);
    }

    [Fact]
    public void New_crash_dumps_are_listed_with_path_and_size_oldest_first()
    {
        var folder = @"C:\ProgramData\se.codeby.rigstats\dumps";
        var dumps = new[]
        {
            new CrashDump("rigstats-sensor.exe.7712.dmp", CrashTime.AddSeconds(2), 3_250_000),
            new CrashDump("rigstats-sensor.exe.4100.dmp", CrashTime.AddDays(-3), 2_000_000), // before the mark
            new CrashDump("rigstats-sensor.exe.6012.dmp", CrashTime.AddHours(-1), 1_048_576),
        };

        var lines = CrashReport.DumpLines(dumps, CrashTime.AddDays(-1), folder);

        Assert.Equal(2, lines.Count);
        Assert.Contains(@"dumps\rigstats-sensor.exe.6012.dmp (1.0 MB, administrators only)", lines[0]);
        Assert.Contains(@"dumps\rigstats-sensor.exe.7712.dmp (3.1 MB, administrators only)", lines[1]);
    }

    [Fact]
    public void The_mark_round_trips_and_a_missing_or_broken_one_is_null()
    {
        var path = Path.Combine(Path.GetTempPath(), $"rigstats-crash-mark-{Guid.NewGuid():N}.txt");
        try
        {
            Assert.Null(CrashReport.ReadMark(path));
            CrashReport.WriteMark(path, CrashTime);
            Assert.Equal(CrashTime, CrashReport.ReadMark(path));
            File.WriteAllText(path, "not a time");
            Assert.Null(CrashReport.ReadMark(path));
        }
        finally
        {
            File.Delete(path);
        }
    }
}
