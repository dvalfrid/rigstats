using System.IO.Pipes;
using System.Text.Json;
using SensorSidecar.Control;
using SensorSidecar.Control.Workers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// The hardware worker (#374): LibreHardwareMonitor in a child process, so a
/// native fault costs a restarted worker, not the service. The protocol and
/// loop run as they do in the child; the client is driven against a worker
/// running that loop in-process, which can crash or hang on request.
/// </summary>
public class HardwareWorkerTests
{
    private sealed class FakeHardware : ISensorHardware
    {
        public List<string> Calls { get; } = [];
        public SensorPayload Sample() => Record("sample", FanSamples.Temps(cpu: 61, gpu: 47, ("System", 33)));
        public string SensorTree() => Record("tree", "# tree\nHW  Cpu");
        public IReadOnlyList<FanHeaderInfo> FanHeaders() =>
            Record("headers", (IReadOnlyList<FanHeaderInfo>)[new FanHeaderInfo("/lpc/nct6799d/0/control/0", "Fan #1", 0, 100)]);
        public void SetFan(string id, double duty) => Record($"set {id} {duty}", 0);
        public void SetFanDefault(string id) => Record($"default {id}", 0);
        public FanReadback ReadFan(string id) =>
            id == "missing" ? throw new InvalidOperationException("no such header") : Record($"read {id}", new FanReadback(true, true, 42.5));
        public void Dispose() { }

        private T Record<T>(string call, T result)
        {
            lock (Calls)
                Calls.Add(call);
            return result;
        }
    }

    /// The worker loop on a thread of this process, over real pipes. `Crash`
    /// makes the next sample end it (output closed, exit code set); `Hang`
    /// makes the next sample never answer.
    private sealed class LoopWorker : IWorker
    {
        private readonly AnonymousPipeServerStream _toWorker = new(PipeDirection.Out);
        private readonly AnonymousPipeServerStream _fromWorker = new(PipeDirection.In);
        private readonly StreamWriter _workerOutput;
        private readonly ManualResetEventSlim _killed = new();
        private int? _exitCode;

        public LoopWorker(ISensorHardware hardware, bool failOpen = false)
        {
            Input = new StreamWriter(_toWorker) { AutoFlush = true };
            Output = new StreamReader(_fromWorker);
            var workerInput = new StreamReader(new AnonymousPipeClientStream(PipeDirection.In, _toWorker.ClientSafePipeHandle));
            _workerOutput = new StreamWriter(new AnonymousPipeClientStream(PipeDirection.Out, _fromWorker.ClientSafePipeHandle)) { AutoFlush = true };
            var thread = new Thread(() =>
            {
                try
                {
                    if (failOpen)
                    {
                        // Opening the hardware failed: every request gets an error.
                        while (workerInput.ReadLine() is { } line)
                            _workerOutput.WriteLine(HardwareWorkerProtocol.Encode(new HardwareWorkerResponse(
                                HardwareWorkerProtocol.DecodeRequest(line).Id, false, Error: "PawnIO not installed")));
                        return;
                    }
                    HardwareWorkerLoop.Run(hardware, workerInput, _workerOutput, BeforeSample);
                }
                catch (Exception)
                {
                    // Killed: its pipes went away.
                }
                finally
                {
                    End(0);
                }
            })
            { IsBackground = true };
            thread.Start();
        }

        public bool Crash { get; set; }
        public bool Hang { get; set; }
        public TextWriter Input { get; }
        public TextReader Output { get; }
        public bool HasExited => _exitCode is not null;
        public int? ExitCode => _exitCode;

        public bool WaitForExit(TimeSpan timeout) => SpinWait.SpinUntil(() => HasExited, timeout);

        public void Kill()
        {
            _killed.Set();
            End(-1);
        }

        public void Dispose() => Kill();

        private void BeforeSample()
        {
            if (Crash)
                End(unchecked((int)0xC0000005));
            if (Hang)
                _killed.Wait();
        }

        private void End(int code)
        {
            lock (_killed)
            {
                if (_exitCode is not null)
                    return;
                _exitCode = code;
            }
            try
            {
                _workerOutput.Dispose();
            }
            catch (IOException)
            {
                // Already broken.
            }
        }
    }

    private sealed class Rig
    {
        public FakeHardware Hardware { get; } = new();
        public List<LoopWorker> Started { get; } = [];
        public long Now { get; set; }
        public bool FailOpen { get; set; }
        public HardwareWorkerClient Client { get; }

        public Rig(TimeSpan? timeout = null) =>
            Client = new HardwareWorkerClient(
                () =>
                {
                    var worker = new LoopWorker(Hardware, FailOpen);
                    Started.Add(worker);
                    return worker;
                },
                timeout: timeout ?? TimeSpan.FromSeconds(5),
                startTimeout: TimeSpan.FromSeconds(5),
                clock: () => Now);

        public LoopWorker Current => Started[^1];
    }

    [Fact]
    public void A_sample_crosses_the_protocol_unchanged()
    {
        var sample = FanSamples.Temps(cpu: 61, gpu: 47, ("System", 33)) with
        {
            GpuDevices = [new GpuDevice("RX 9070 XT", "gpu-amd", 12, 47, 60, 70, 2400, 1250, 230, 35, 4096, 16304, 10, null)],
        };

        var back = HardwareWorkerProtocol.FromJson<SensorPayload>(HardwareWorkerProtocol.ToJson(sample));

        Assert.Equal(JsonSerializer.Serialize(sample, HardwareHost.TelemetryJsonOptions),
            JsonSerializer.Serialize(back, HardwareHost.TelemetryJsonOptions));
    }

    [Fact]
    public void Every_operation_goes_through_one_worker()
    {
        var rig = new Rig();
        var c = rig.Client;

        Assert.Equal(61, c.Sample().CpuTemp);
        Assert.StartsWith("# tree", c.SensorTree());
        Assert.Equal("Fan #1", Assert.Single(c.FanHeaders()).Label);
        c.SetFan("/lpc/nct6799d/0/control/0", 40);
        c.SetFanDefault("/lpc/nct6799d/0/control/0");
        Assert.Equal(new FanReadback(true, true, 42.5), c.ReadFan("/lpc/nct6799d/0/control/0"));

        Assert.Single(rig.Started);
        Assert.Equal(
            ["sample", "tree", "headers", "set /lpc/nct6799d/0/control/0 40", "default /lpc/nct6799d/0/control/0", "read /lpc/nct6799d/0/control/0"],
            rig.Hardware.Calls);
    }

    [Fact]
    public void A_hardware_error_is_an_error_not_a_crash()
    {
        var rig = new Rig();

        var e = Assert.Throws<InvalidOperationException>(() => rig.Client.ReadFan("missing"));

        Assert.IsNotType<HardwareWorkerDownException>(e);
        Assert.Contains("no such header", e.Message);
        Assert.False(rig.Current.HasExited);
        Assert.Equal(61, rig.Client.Sample().CpuTemp); // same worker, still fine
        Assert.Single(rig.Started);
    }

    [Fact]
    public void A_crash_is_reported_and_the_next_call_starts_a_new_worker_at_once()
    {
        var rig = new Rig();
        rig.Client.Sample();
        rig.Current.Crash = true;

        Assert.Throws<HardwareWorkerDownException>(() => rig.Client.Sample());

        // The first restart is immediate: sensors and fans can't wait.
        Assert.Equal(61, rig.Client.Sample().CpuTemp);
        Assert.Equal(2, rig.Started.Count);
    }

    [Fact]
    public void A_worker_that_keeps_crashing_is_restarted_with_growing_pauses()
    {
        var rig = new Rig();

        // Crash #1: the restart is immediate. Crash #2: 1 s pause.
        rig.Client.Sample();
        rig.Current.Crash = true;
        Assert.Throws<HardwareWorkerDownException>(() => rig.Client.Sample());
        rig.Client.Sample();
        rig.Current.Crash = true;
        Assert.Throws<HardwareWorkerDownException>(() => rig.Client.Sample());

        // Within the pause: no new worker, the call fails at once.
        rig.Now += 500;
        Assert.Throws<HardwareWorkerDownException>(() => rig.Client.Sample());
        Assert.Equal(2, rig.Started.Count);

        rig.Now += 600;
        Assert.Equal(61, rig.Client.Sample().CpuTemp);
        Assert.Equal(3, rig.Started.Count);
    }

    [Fact]
    public void A_worker_that_ran_a_while_starts_the_count_over()
    {
        var rig = new Rig();
        for (var i = 0; i < 4; i++)
        {
            rig.Client.Sample();
            rig.Now += (long)HardwareWorkerClient.StableAfter.TotalMilliseconds;
            rig.Current.Crash = true;
            Assert.Throws<HardwareWorkerDownException>(() => rig.Client.Sample());
            // Each crash came after a stable run: restart at once, every time.
            Assert.Equal(61, rig.Client.Sample().CpuTemp);
        }
    }

    [Fact]
    public void A_hanging_worker_is_ended_after_the_time_limit()
    {
        var rig = new Rig(timeout: TimeSpan.FromMilliseconds(300));
        rig.Client.Sample();
        rig.Current.Hang = true;

        Assert.Throws<HardwareWorkerDownException>(() => rig.Client.Sample());

        Assert.True(rig.Started[0].HasExited);
        rig.Current.Hang = false;
        Assert.Equal(61, rig.Client.Sample().CpuTemp);
        Assert.Equal(2, rig.Started.Count);
    }

    [Fact]
    public void Restart_ends_the_worker_even_while_a_call_hangs_in_it()
    {
        // The fan watchdog's action: the hung call holds the client's lock,
        // so Restart works without it.
        var rig = new Rig(timeout: TimeSpan.FromSeconds(30));
        rig.Client.Sample();
        rig.Current.Hang = true;
        var hung = Task.Run(() => rig.Client.Sample());
        Thread.Sleep(200);

        rig.Client.Restart("fan loop stalled for 10 s");

        Assert.Throws<HardwareWorkerDownException>(() => hung.GetAwaiter().GetResult());
        Assert.Equal(61, rig.Client.Sample().CpuTemp);
        Assert.Equal(2, rig.Started.Count);
    }

    [Fact]
    public void A_worker_that_cannot_open_the_hardware_is_down_and_tried_again_later()
    {
        var rig = new Rig { FailOpen = true };

        var e = Assert.Throws<HardwareWorkerDownException>(() => rig.Client.Sample());
        Assert.Contains("PawnIO not installed", e.Message);

        rig.FailOpen = false;
        rig.Now += 5_000;
        Assert.Equal(61, rig.Client.Sample().CpuTemp);
    }

    [Fact]
    public void Dispose_ends_the_worker_cleanly()
    {
        var rig = new Rig();
        rig.Client.Sample();

        rig.Client.Dispose();

        Assert.True(SpinWait.SpinUntil(() => rig.Current.HasExited, TimeSpan.FromSeconds(5)));
    }
}
