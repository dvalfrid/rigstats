using System.Collections.Concurrent;
using System.Text.Json.Nodes;
using SensorSidecar.Control;
using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// The GPU worker process (#370): the GPU driver libraries run in a child
/// process, so a crash or a hang in a vendor DLL costs only the GPU power
/// limits — never the service. These tests pin that a crash, a hang, a
/// garbled answer, a worker that won't start and a crash in an earlier
/// service process all end up as a logged, explained "unavailable", and
/// that the real process plumbing (pipes, exit codes, killing, UTF-8) works.
/// </summary>
public sealed class GpuWorkerTests : IDisposable
{
    private const string Id = "PCI\\VEN_1002&DEV_7550";
    private const int AccessViolation = unchecked((int)0xC0000005);
    private static readonly GpuPowerAdapter Rx9070Xt = new(Id, "AMD Radeon RX 9070 XT", -30, 10, 1, 0);
    private static readonly DateTimeOffset Boot = new(2026, 10, 15, 16, 31, 35, TimeSpan.Zero);
    private const string Drivers = "amdadlx64.dll 1.5.0.124; nvml.dll -";

    private readonly string _dir = Path.Combine(Path.GetTempPath(), "rigstats-gpuworker-" + Guid.NewGuid().ToString("N"));
    private string Marker => Path.Combine(_dir, "gpu-worker-crash.marker");

    public GpuWorkerTests() => Directory.CreateDirectory(_dir);

    public void Dispose()
    {
        try
        {
            Directory.Delete(_dir, recursive: true);
        }
        catch
        {
            // Best effort.
        }
    }

    // ---- fakes ----------------------------------------------------------

    private sealed class FakeApi(params GpuPowerAdapter[] adapters) : IGpuPowerApi
    {
        public Dictionary<string, int> Limits { get; } = adapters.ToDictionary(a => a.Id, _ => 0);
        public HashSet<string> AtFactory { get; } = adapters.Select(a => a.Id).ToHashSet();
        public int EndSessions { get; private set; }
        public Exception? Failure { get; set; }

        public IReadOnlyList<GpuPowerAdapter> Adapters() => Failure is null ? adapters : throw Failure;
        public int GetPowerLimit(string adapterId) => Failure is null ? Limits[adapterId] : throw Failure;

        public void SetPowerLimit(string adapterId, int percent)
        {
            if (Failure is not null)
                throw Failure;
            Limits[adapterId] = percent;
            AtFactory.Remove(adapterId);
        }

        public bool IsAtFactory(string adapterId) => AtFactory.Contains(adapterId);

        public void ResetToFactory(string adapterId)
        {
            Limits[adapterId] = 0;
            AtFactory.Add(adapterId);
        }

        public void EndSession() => EndSessions++;
    }

    /// What the scripted worker does with one request.
    private sealed record Reply(string? Line = null, bool Crash = false, bool Hang = false, int ExitCode = AccessViolation, TimeSpan Delay = default)
    {
        public static Reply From(GpuWorkerResponse response) => new(GpuWorkerProtocol.Encode(response));
    }

    /// A worker without a process: each request line is answered by
    /// `answer` — a line, a crash (output closes, exit code set) or nothing
    /// (a hang, until killed).
    private sealed class ScriptedWorker : IGpuWorker
    {
        private readonly BlockingCollection<string?> _output = new();
        private readonly Func<GpuWorkerRequest, Reply> _answer;

        public ScriptedWorker(Func<GpuWorkerRequest, Reply> answer)
        {
            _answer = answer;
            Input = new LineWriter(OnLine);
            Output = new QueueReader(_output);
        }

        public List<GpuWorkerRequest> Requests { get; } = [];
        public bool Killed { get; private set; }
        public bool Disposed { get; private set; }
        public TextWriter Input { get; }
        public TextReader Output { get; }
        public bool HasExited { get; private set; }
        public int? ExitCode { get; private set; }

        public bool WaitForExit(TimeSpan timeout) => HasExited;

        public void Kill()
        {
            Killed = true;
            Exit(1);
        }

        public void Die(int code) => Exit(code);

        public void Dispose()
        {
            Disposed = true;
            if (!HasExited)
                _output.Add(null); // lets the client's reader thread finish
        }

        private void Exit(int code)
        {
            if (HasExited)
                return;
            HasExited = true;
            ExitCode = code;
            _output.Add(null);
        }

        private void OnLine(string line)
        {
            var request = GpuWorkerProtocol.DecodeRequest(line);
            Requests.Add(request);
            var reply = _answer(request);
            if (reply.Crash)
                Exit(reply.ExitCode);
            else if (reply.Delay > TimeSpan.Zero)
                Task.Delay(reply.Delay).ContinueWith(_ => _output.Add(reply.Line));
            else if (!reply.Hang)
                _output.Add(reply.Line);
            if (request.Op == GpuWorkerProtocol.OpEnd && !reply.Crash && !reply.Hang)
                Exit(0);
        }

        private sealed class LineWriter(Action<string> onLine) : StringWriter
        {
            public override void WriteLine(string? value) => onLine(value ?? "");
        }

        private sealed class QueueReader(BlockingCollection<string?> lines) : TextReader
        {
            public override string? ReadLine() => lines.Take();
        }
    }

    /// Starts scripted workers and counts the starts.
    private sealed class Starter(Func<GpuWorkerRequest, Reply> answer)
    {
        public List<ScriptedWorker> Started { get; } = [];
        public Exception? StartFailure { get; set; }

        public IGpuWorker Start()
        {
            if (StartFailure is not null)
                throw StartFailure;
            var worker = new ScriptedWorker(answer);
            Started.Add(worker);
            return worker;
        }
    }

    private static Func<GpuWorkerRequest, Reply> Real(IGpuPowerApi api) => r => Reply.From(GpuWorkerLoop.Handle(api, r));

    /// `timeout` sets both limits unless `startTimeout` is given.
    private GpuWorkerClient Client(
        Starter starter,
        DateTimeOffset? boot = null,
        string drivers = Drivers,
        TimeSpan? timeout = null,
        TimeSpan? startTimeout = null,
        Func<long>? clock = null) =>
        new(starter.Start, Marker, boot ?? Boot, drivers, timeout, startTimeout ?? timeout, clock: clock);

    // ---- protocol -------------------------------------------------------

    [Fact]
    public void Requests_and_answers_round_trip()
    {
        var request = new GpuWorkerRequest(7, GpuWorkerProtocol.OpSet, Id, -12);
        Assert.Equal(request, GpuWorkerProtocol.DecodeRequest(GpuWorkerProtocol.Encode(request)));

        var answer = GpuWorkerProtocol.DecodeResponse(GpuWorkerProtocol.Encode(
            new GpuWorkerResponse(7, true, new JsonArray(GpuWorkerProtocol.ToJson(Rx9070Xt)))));
        Assert.True(answer.Ok);
        Assert.Equal(Rx9070Xt, GpuWorkerProtocol.FromJson(answer.Result!.AsArray()[0]!));

        var error = GpuWorkerProtocol.DecodeResponse(GpuWorkerProtocol.Encode(new GpuWorkerResponse(8, false, Error: "ADLX_FAIL")));
        Assert.Equal((8, false, "ADLX_FAIL"), (error.Id, error.Ok, error.Error));
    }

    [Fact]
    public void Exit_codes_read_as_windows_status_codes()
    {
        Assert.Equal("0xC0000005", GpuWorkerClient.FormatExitCode(AccessViolation));
        Assert.Equal("0x00000003", GpuWorkerClient.FormatExitCode(3));
        Assert.Equal("unknown", GpuWorkerClient.FormatExitCode(null));
    }

    // ---- the worker's loop ----------------------------------------------

    private static string Lines(params GpuWorkerRequest[] requests) =>
        string.Join("\n", requests.Select(GpuWorkerProtocol.Encode)) + "\n";

    private static List<GpuWorkerResponse> Answers(StringWriter output) =>
        output.ToString().Split('\n', StringSplitOptions.RemoveEmptyEntries).Select(l => GpuWorkerProtocol.DecodeResponse(l.Trim())).ToList();

    [Fact]
    public void The_loop_answers_each_request_and_stops_at_end()
    {
        var api = new FakeApi(Rx9070Xt);
        var output = new StringWriter();

        GpuWorkerLoop.Run(api, new StringReader(Lines(
            new(1, GpuWorkerProtocol.OpAdapters),
            new(2, GpuWorkerProtocol.OpSet, Id, -10),
            new(3, GpuWorkerProtocol.OpGet, Id),
            new(4, GpuWorkerProtocol.OpIsAtFactory, Id),
            new(5, GpuWorkerProtocol.OpEnd),
            new(6, GpuWorkerProtocol.OpAdapters))), output); // after "end": not read

        var answers = Answers(output);
        Assert.Equal([1, 2, 3, 4, 5], answers.Select(a => a.Id));
        Assert.All(answers, a => Assert.True(a.Ok));
        Assert.Equal(Rx9070Xt, GpuWorkerProtocol.FromJson(answers[0].Result!.AsArray()[0]!));
        Assert.Equal(-10, (int)answers[2].Result!);
        Assert.False((bool)answers[3].Result!);
        Assert.Equal(-10, api.Limits[Id]);
        Assert.True(api.EndSessions >= 1);
    }

    [Fact]
    public void A_driver_failure_is_an_error_answer_and_the_loop_keeps_going()
    {
        var api = new FakeApi(Rx9070Xt) { Failure = new InvalidOperationException("ADLX ADLXInitialize failed (ADLX_FAIL).") };
        var output = new StringWriter();

        GpuWorkerLoop.Run(api, new StringReader(Lines(new GpuWorkerRequest(1, GpuWorkerProtocol.OpAdapters), new(2, GpuWorkerProtocol.OpIsAtFactory, Id))), output);

        var answers = Answers(output);
        Assert.Equal((false, "ADLX ADLXInitialize failed (ADLX_FAIL)."), (answers[0].Ok, answers[0].Error));
        Assert.True(answers[1].Ok);
    }

    [Fact]
    public void An_unreadable_request_gets_an_error_answer_and_the_loop_keeps_going()
    {
        var output = new StringWriter();

        GpuWorkerLoop.Run(new FakeApi(Rx9070Xt), new StringReader("not json\n" + Lines(new GpuWorkerRequest(2, GpuWorkerProtocol.OpAdapters))), output);

        var answers = Answers(output);
        Assert.False(answers[0].Ok);
        Assert.Contains("Unreadable request", answers[0].Error);
        Assert.True(answers[1].Ok);
    }

    [Fact]
    public void Without_a_driver_there_are_no_adapters_and_the_rest_is_refused()
    {
        var output = new StringWriter();

        GpuWorkerLoop.Run(null, new StringReader(Lines(new GpuWorkerRequest(1, GpuWorkerProtocol.OpAdapters), new(2, GpuWorkerProtocol.OpGet, Id))), output);

        var answers = Answers(output);
        Assert.Empty(answers[0].Result!.AsArray());
        Assert.False(answers[1].Ok);
    }

    [Fact]
    public void The_driver_session_ends_when_the_service_goes_away()
    {
        var api = new FakeApi(Rx9070Xt);

        GpuWorkerLoop.Run(api, new StringReader(Lines(new GpuWorkerRequest(1, GpuWorkerProtocol.OpAdapters))), new StringWriter()); // EOF, no "end"

        Assert.Equal(1, api.EndSessions);
    }

    [Fact]
    public void A_request_for_an_adapter_needs_one()
    {
        var answer = GpuWorkerLoop.Handle(new FakeApi(Rx9070Xt), new GpuWorkerRequest(1, GpuWorkerProtocol.OpSet, null, 5));

        Assert.False(answer.Ok);
        Assert.Contains("needs an adapter", answer.Error);
    }

    [Fact]
    public void The_test_hook_runs_once_before_the_first_request()
    {
        var calls = 0;

        GpuWorkerLoop.Run(new FakeApi(Rx9070Xt), new StringReader(Lines(new GpuWorkerRequest(1, GpuWorkerProtocol.OpAdapters), new(2, GpuWorkerProtocol.OpAdapters))),
            new StringWriter(), () => calls++);

        Assert.Equal(1, calls);
    }

    // ---- the service's client -------------------------------------------

    [Fact]
    public void Calls_go_through_one_worker()
    {
        var api = new FakeApi(Rx9070Xt);
        var starter = new Starter(Real(api));
        using var client = Client(starter);

        Assert.Equal([Rx9070Xt], client.Adapters());
        client.SetPowerLimit(Id, -8);
        Assert.Equal(-8, client.GetPowerLimit(Id));
        Assert.False(client.IsAtFactory(Id));
        client.ResetToFactory(Id);
        Assert.True(client.IsAtFactory(Id));

        Assert.Single(starter.Started);
        Assert.Equal(6, starter.Started[0].Requests.Count);
    }

    [Fact]
    public void A_driver_error_is_an_error_not_a_crash()
    {
        var api = new FakeApi(Rx9070Xt) { Failure = new InvalidOperationException("ADLX_FAIL") };
        var starter = new Starter(Real(api));
        using var client = Client(starter);

        var e = Assert.Throws<InvalidOperationException>(() => client.Adapters());
        Assert.IsNotType<GpuDriverCrashedException>(e);
        Assert.Equal("ADLX_FAIL", e.Message);

        api.Failure = null; // the driver recovers: the same worker answers
        Assert.Equal([Rx9070Xt], client.Adapters());
        Assert.Single(starter.Started);
        Assert.False(File.Exists(Marker));
    }

    [Fact]
    public void A_crash_turns_the_gpu_off_with_the_exit_code_and_is_remembered()
    {
        var starter = new Starter(_ => new Reply(Crash: true));
        using var client = Client(starter);

        var e = Assert.Throws<GpuDriverCrashedException>(() => client.Adapters());
        Assert.Contains("graphics driver crashed (exit code 0xC0000005)", e.Message);
        Assert.Contains("tried again after Windows restarts", e.Message);

        // Every later call says the same, without starting another worker.
        Assert.Equal(e.Message, Assert.Throws<GpuDriverCrashedException>(() => client.GetPowerLimit(Id)).Message);
        Assert.Single(starter.Started);
        Assert.True(starter.Started[0].Disposed);
        Assert.Equal(e.Message, GpuCrashGuard.CrashedBefore(File.ReadAllText(Marker), Boot, Drivers));
    }

    [Fact]
    public void A_new_service_process_honours_a_crash_from_this_boot()
    {
        using (var crashed = Client(new Starter(_ => new Reply(Crash: true))))
            Assert.Throws<GpuDriverCrashedException>(() => crashed.Adapters());

        var starter = new Starter(Real(new FakeApi(Rx9070Xt)));
        using var client = Client(starter);

        Assert.Contains("crashed", Assert.Throws<GpuDriverCrashedException>(() => client.Adapters()).Message);
        Assert.Empty(starter.Started);
    }

    [Fact]
    public void A_restart_or_a_new_driver_gets_a_new_try()
    {
        using (var crashed = Client(new Starter(_ => new Reply(Crash: true))))
            Assert.Throws<GpuDriverCrashedException>(() => crashed.Adapters());

        var afterRestart = new Starter(Real(new FakeApi(Rx9070Xt)));
        using (var client = Client(afterRestart, boot: Boot.AddHours(2)))
            Assert.Equal([Rx9070Xt], client.Adapters());

        var newDriver = new Starter(Real(new FakeApi(Rx9070Xt)));
        using (var client = Client(newDriver, drivers: "amdadlx64.dll 1.5.0.130; nvml.dll -"))
            Assert.Equal([Rx9070Xt], client.Adapters());

        Assert.Single(afterRestart.Started);
        Assert.Single(newDriver.Started);
    }

    [Fact]
    public void A_hanging_worker_is_ended_after_the_time_limit()
    {
        var starter = new Starter(_ => new Reply(Hang: true));
        using var client = Client(starter, timeout: TimeSpan.FromMilliseconds(300));

        var e = Assert.Throws<GpuDriverCrashedException>(() => client.Adapters());

        Assert.Contains("did not answer within 0 s", e.Message); // 0.3 s, rounded
        Assert.Contains("tried again in 10 minutes", e.Message);
        Assert.True(e.Hung);
        Assert.True(starter.Started[0].Killed);
        Assert.False(File.Exists(Marker)); // a hang isn't remembered past this process
    }

    [Fact]
    public void After_a_hang_the_worker_is_tried_again_ten_minutes_later()
    {
        long now = 1_000_000;
        var hang = true;
        var api = new FakeApi(Rx9070Xt);
        var starter = new Starter(r => hang ? new Reply(Hang: true) : Reply.From(GpuWorkerLoop.Handle(api, r)));
        using var client = Client(starter, timeout: TimeSpan.FromMilliseconds(200), clock: () => now);
        Assert.Throws<GpuDriverCrashedException>(() => client.Adapters());
        hang = false;

        now += (long)TimeSpan.FromMinutes(9).TotalMilliseconds;
        Assert.Throws<GpuDriverCrashedException>(() => client.Adapters()); // still paused
        Assert.Single(starter.Started);

        now += (long)TimeSpan.FromMinutes(2).TotalMilliseconds;
        Assert.Equal([Rx9070Xt], client.Adapters()); // a new worker, and it answers
        Assert.Equal(2, starter.Started.Count);
    }

    [Fact]
    public void A_crash_is_not_retried_after_ten_minutes()
    {
        long now = 1_000_000;
        var starter = new Starter(_ => new Reply(Crash: true));
        using var client = Client(starter, clock: () => now);
        Assert.Throws<GpuDriverCrashedException>(() => client.Adapters());

        now += (long)TimeSpan.FromHours(5).TotalMilliseconds;

        Assert.Throws<GpuDriverCrashedException>(() => client.Adapters());
        Assert.Single(starter.Started);
    }

    [Fact]
    public void The_first_call_of_a_new_worker_gets_the_longer_start_limit()
    {
        var api = new FakeApi(Rx9070Xt);
        // Every answer takes 400 ms: fine while the worker starts, too slow after.
        var starter = new Starter(r => new Reply(GpuWorkerProtocol.Encode(GpuWorkerLoop.Handle(api, r)), Delay: TimeSpan.FromMilliseconds(400)));
        using var client = Client(starter, timeout: TimeSpan.FromMilliseconds(150), startTimeout: TimeSpan.FromSeconds(5));

        Assert.Equal([Rx9070Xt], client.Adapters()); // 400 ms is within the start limit

        var e = Assert.Throws<GpuDriverCrashedException>(() => client.Adapters()); // but not the normal one
        Assert.True(e.Hung);
    }

    [Fact]
    public void A_worker_that_died_between_calls_counts_as_a_crash()
    {
        var starter = new Starter(Real(new FakeApi(Rx9070Xt)));
        using var client = Client(starter);
        client.Adapters();

        starter.Started[0].Die(AccessViolation);

        Assert.Contains("0xC0000005", Assert.Throws<GpuDriverCrashedException>(() => client.Adapters()).Message);
        Assert.Single(starter.Started);
    }

    [Fact]
    public void A_garbled_answer_restarts_the_worker_without_blaming_the_driver()
    {
        var garble = true;
        var api = new FakeApi(Rx9070Xt);
        var starter = new Starter(r => garble ? new Reply("{not json") : Reply.From(GpuWorkerLoop.Handle(api, r)));
        using var client = Client(starter);

        var e = Assert.Throws<InvalidOperationException>(() => client.Adapters());
        Assert.IsNotType<GpuDriverCrashedException>(e);
        Assert.True(starter.Started[0].Killed);
        Assert.False(File.Exists(Marker));

        garble = false;
        Assert.Equal([Rx9070Xt], client.Adapters());
        Assert.Equal(2, starter.Started.Count);
    }

    [Fact]
    public void An_answer_to_another_request_is_garbled()
    {
        var starter = new Starter(r => Reply.From(new GpuWorkerResponse(r.Id + 100, true, new JsonArray())));
        using var client = Client(starter);

        Assert.IsNotType<GpuDriverCrashedException>(Assert.Throws<InvalidOperationException>(() => client.Adapters()));
        Assert.True(starter.Started[0].Killed);
    }

    [Fact]
    public void An_unreadable_result_is_an_error_not_a_crash()
    {
        var starter = new Starter(r => Reply.From(new GpuWorkerResponse(r.Id, true, "not a number")));
        using var client = Client(starter);

        var e = Assert.Throws<InvalidOperationException>(() => client.GetPowerLimit(Id));
        Assert.IsNotType<GpuDriverCrashedException>(e);
        Assert.Contains("unreadable", e.Message);
    }

    [Fact]
    public void End_session_ends_the_worker_and_the_next_call_starts_a_new_one()
    {
        var api = new FakeApi(Rx9070Xt);
        var starter = new Starter(Real(api));
        var client = Client(starter);
        client.Adapters();

        client.EndSession();

        Assert.Equal(GpuWorkerProtocol.OpEnd, starter.Started[0].Requests[^1].Op);
        Assert.True(starter.Started[0].Disposed);
        Assert.Equal([Rx9070Xt], client.Adapters());
        Assert.Equal(2, starter.Started.Count);
        client.Dispose();
    }

    [Fact]
    public void End_session_never_throws()
    {
        var hanging = new Starter(r => r.Op == GpuWorkerProtocol.OpEnd ? new Reply(Hang: true) : Reply.From(GpuWorkerLoop.Handle(new FakeApi(Rx9070Xt), r)));
        var client = Client(hanging);
        client.Adapters();

        client.EndSession(); // the worker doesn't answer "end": it is killed, nothing thrown
        Assert.True(hanging.Started[0].Killed);

        var crashed = Client(new Starter(_ => new Reply(Crash: true)));
        Assert.Throws<GpuDriverCrashedException>(() => crashed.Adapters());
        crashed.EndSession();             // nothing running
        Client(hanging).EndSession();     // never started
    }

    [Fact]
    public void A_worker_that_cannot_start_is_unavailable_but_not_remembered()
    {
        var starter = new Starter(Real(new FakeApi(Rx9070Xt))) { StartFailure = new System.ComponentModel.Win32Exception(2) };
        using var client = Client(starter);

        var e = Assert.Throws<InvalidOperationException>(() => client.Adapters());
        Assert.IsNotType<GpuDriverCrashedException>(e);
        Assert.False(File.Exists(Marker));

        starter.StartFailure = null; // e.g. a file lock let go: tried again
        Assert.Equal([Rx9070Xt], client.Adapters());
    }

    [Fact]
    public void Calls_from_several_threads_share_one_worker_safely()
    {
        var api = new FakeApi(Rx9070Xt);
        var starter = new Starter(Real(api));
        using var client = Client(starter);

        Parallel.For(0, 50, i => Assert.Equal([Rx9070Xt], client.Adapters()));

        Assert.Single(starter.Started);
        Assert.Equal(50, starter.Started[0].Requests.Count);
        Assert.Equal(50, starter.Started[0].Requests.Select(r => r.Id).Distinct().Count());
    }

    // ---- through the GPU provider ---------------------------------------

    [Fact]
    public void The_gpu_tab_works_through_the_worker()
    {
        var api = new FakeApi(Rx9070Xt);
        using var client = Client(new Starter(Real(api)));

        var caps = new GpuPowerProvider(client, dryRun: false, Path.Combine(_dir, "originals.json")).Probe();

        Assert.True(caps.Supported);
        Assert.Equal(Id, (string)caps.Details!["adapters"]![0]!["id"]!);
    }

    [Fact]
    public void A_crashing_driver_only_turns_the_gpu_tab_off()
    {
        using var client = Client(new Starter(_ => new Reply(Crash: true)));
        var provider = new GpuPowerProvider(client, dryRun: false, Path.Combine(_dir, "originals.json"));

        var caps = provider.Probe();
        provider.ReleaseToFirmware(); // service stop: nothing to restore, nothing thrown

        Assert.False(caps.Supported);
        Assert.Contains("graphics driver crashed (exit code 0xC0000005)", caps.Reason);
    }

    // ---- what the user sees ---------------------------------------------

    private const string AmdOnly = "amdadlx64.dll 1.5.0.124; nvml.dll -";
    private const string NvidiaOnly = "amdadlx64.dll -; nvml.dll 32.0.15.6094";

    private static GpuDriverCrashedException Crashed() =>
        new("GPU power limits are off: the graphics driver crashed (exit code 0xC0000005). They are tried again after Windows restarts or the graphics driver is updated.");

    private static GpuDriverCrashedException Hung() =>
        new("GPU power limits are off: the graphics driver did not answer within 20 s. They are tried again in 10 minutes.");

    [Fact]
    public void A_crash_explains_itself_and_offers_the_driver_diagnostics_and_help()
    {
        var problem = GpuProblems.DriverDown(Crashed(), AmdOnly, limitChanged: false);

        Assert.Equal("gpu_driver_crashed", problem.Id);
        Assert.Equal(ProblemSeverity.Error, problem.Severity);
        Assert.Equal("The graphics driver crashed", problem.Title);
        Assert.Contains("Sensors, fans and lighting keep working", problem.Message);
        Assert.Contains("install the latest driver, then restart Windows", problem.Message);
        Assert.DoesNotContain("RIGStats had changed", problem.Message);
        Assert.Equal(
            [
                (ProblemActionKind.OpenUrl, "Get the AMD driver", GpuProblems.AmdDriverUrl),
                (ProblemActionKind.CollectDiagnostics, "Collect diagnostics…", null),
                (ProblemActionKind.OpenUrl, "Read more", GpuProblems.AmdHelpUrl),
            ],
            problem.Actions.Select(a => (a.Kind, a.Label, a.Target)));
        Assert.Contains("0xC0000005", problem.Detail);
        Assert.Contains(AmdOnly, problem.Detail);
    }

    [Fact]
    public void A_hang_says_the_driver_stopped_answering()
    {
        var problem = GpuProblems.DriverDown(Hung(), AmdOnly, limitChanged: false);

        Assert.Equal("gpu_driver_hung", problem.Id);
        Assert.Equal("The graphics driver stopped answering", problem.Title);
        Assert.Contains("the driver stopped answering", problem.Message);
        Assert.Contains("tries again in a few minutes", problem.Message);
        Assert.DoesNotContain("after a restart", problem.Message);
    }

    [Fact]
    public void An_nvidia_pc_gets_the_nvidia_driver_and_the_general_help()
    {
        var problem = GpuProblems.DriverDown(Crashed(), NvidiaOnly, limitChanged: false);

        Assert.Equal(
            ["Get the NVIDIA driver", "Collect diagnostics…", "Read more"],
            problem.Actions.Select(a => a.Label));
        Assert.Equal(GpuProblems.HelpUrl, problem.Actions[^1].Target);
    }

    [Fact]
    public void A_power_limit_rigstats_changed_is_mentioned()
    {
        Assert.Contains("RIGStats had changed the power limit", GpuProblems.DriverDown(Crashed(), AmdOnly, limitChanged: true).Message);
        Assert.Contains("RIGStats had changed the power limit", GpuProblems.DriverFailed("ADLX_FAIL", AmdOnly, limitChanged: true).Message);
    }

    [Fact]
    public void A_driver_error_is_attention_not_an_error()
    {
        var problem = GpuProblems.DriverFailed("ADLX ADLXInitialize failed (ADLX_FAIL).", AmdOnly, limitChanged: false);

        Assert.Equal(("gpu_driver_failed", ProblemSeverity.Attention), (problem.Id, problem.Severity));
        Assert.Contains("ADLX_FAIL", problem.Detail);
        Assert.Contains(problem.Actions, a => a.Kind == ProblemActionKind.CollectDiagnostics);
    }

    [Theory]
    [InlineData("amdadlx64.dll 1.5.0.124; nvml.dll -", "amdadlx64.dll", true)]
    [InlineData("amdadlx64.dll 1.5.0.124; nvml.dll -", "nvml.dll", false)]
    [InlineData("amdadlx64.dll -; nvml.dll 32.0.15.6094", "nvml.dll", true)]
    [InlineData("", "nvml.dll", false)]
    public void Installed_reads_the_driver_files(string drivers, string file, bool installed) =>
        Assert.Equal(installed, GpuProblems.Installed(drivers, file));

    [Fact]
    public void The_gpu_capability_carries_the_problem()
    {
        using var client = Client(new Starter(_ => new Reply(Crash: true)));

        var caps = new GpuPowerProvider(client, dryRun: false, Path.Combine(_dir, "originals.json"), () => AmdOnly).Probe();

        Assert.False(caps.Supported);
        Assert.Equal("gpu_driver_crashed", caps.Problem?.Id);
        Assert.Equal(ProblemSeverity.Error, caps.Problem?.Severity);
    }

    [Fact]
    public void A_driver_that_answers_with_an_error_is_attention_in_the_capability()
    {
        var api = new FakeApi(Rx9070Xt) { Failure = new InvalidOperationException("ADLX_FAIL") };
        using var client = Client(new Starter(Real(api)));

        var caps = new GpuPowerProvider(client, dryRun: false, Path.Combine(_dir, "originals.json"), () => AmdOnly).Probe();

        Assert.Equal("gpu_driver_failed", caps.Problem?.Id);
    }

    [Fact]
    public void A_pc_without_a_gpu_driver_has_no_problem_just_no_tab()
    {
        var caps = new GpuPowerProvider(null, dryRun: false, Path.Combine(_dir, "originals.json")).Probe();

        Assert.False(caps.Supported);
        Assert.Null(caps.Problem);
    }

    /// The problem's wire shape, pinned by the example the Rust reader parses
    /// too (`control.rs`, `capability_problem_contract_parses`) — neither side
    /// can rename a field alone. Regenerate with
    /// <c>$env:RIGSTATS_UPDATE_CONTRACT=1; dotnet test --filter Capability_problem</c>.
    [Fact]
    public void Capability_problem_serializes_as_the_shared_contract_example()
    {
        var caps = new SensorSidecar.Control.CapabilitySet
        {
            Domain = "gpu",
            Supported = false,
            Reason = Crashed().Message,
            Problem = GpuProblems.DriverDown(Crashed(), AmdOnly, limitChanged: false),
        };
        var json = System.Text.Json.JsonSerializer.Serialize(caps, SensorSidecar.Control.ControlJson.Options);
        var path = Path.Combine(RepoRoot(), "sensor-sidecar.Tests", "contract", "control-capability-problem.json");

        if (Environment.GetEnvironmentVariable("RIGSTATS_UPDATE_CONTRACT") == "1")
            File.WriteAllText(path, json + "\n");
        Assert.Equal(File.ReadAllText(path).Trim(), json);
    }

    private static string RepoRoot()
    {
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
            if (Directory.Exists(Path.Combine(dir.FullName, "sensor-sidecar.Tests", "contract")))
                return dir.FullName;
        throw new DirectoryNotFoundException("repo root with sensor-sidecar.Tests/contract");
    }

    // ---- the crash guard ------------------------------------------------

    [Fact]
    public void The_crash_guard_matches_only_this_boot_and_these_drivers()
    {
        const string reason = "GPU power limits are off: the graphics driver crashed (exit code 0xC0000005). Tried again | later.";
        var marker = GpuCrashGuard.Marker(Boot, Drivers, reason);

        Assert.Equal(reason, GpuCrashGuard.CrashedBefore(marker, Boot, Drivers));          // a '|' in the reason survives
        Assert.Equal(reason, GpuCrashGuard.CrashedBefore(marker, Boot.AddSeconds(40), Drivers));
        Assert.Null(GpuCrashGuard.CrashedBefore(marker, Boot.AddHours(3), Drivers));       // restarted since
        Assert.Null(GpuCrashGuard.CrashedBefore(marker, Boot, "amdadlx64.dll 1.5.0.130; nvml.dll -")); // new driver
        Assert.Null(GpuCrashGuard.CrashedBefore(null, Boot, Drivers));
        Assert.Null(GpuCrashGuard.CrashedBefore("garbage", Boot, Drivers));
        Assert.Null(GpuCrashGuard.CrashedBefore("not-a-date|x|y", Boot, Drivers));
    }

    [Fact]
    public void Driver_files_are_named_without_loading_them()
    {
        var drivers = GpuCrashGuard.DriverFiles();

        Assert.Contains("amdadlx64.dll ", drivers);
        Assert.Contains("nvml.dll ", drivers);
    }

    // ---- real processes ---------------------------------------------------

    private static string Cmd => Path.Combine(Environment.SystemDirectory, "cmd.exe");

    [Fact]
    public void A_real_worker_process_carries_utf8_lines_without_a_byte_order_mark()
    {
        // findstr "^" echoes every line it reads: what we write comes back as is.
        using var worker = ProcessGpuWorker.Start(Cmd, "/d /c findstr \"^\"", _ => { });
        const string line = "{\"name\":\"AMD Radeon™ RX 9070 XT\"}";

        worker.Input.WriteLine(line);
        worker.Input.Close(); // findstr buffers its output until its input ends

        var read = Task.Run(worker.Output.ReadLine);
        Assert.True(read.Wait(TimeSpan.FromSeconds(10)), "no echo within 10 s");
        Assert.Equal(line, read.Result);
    }

    [Fact]
    public void A_real_worker_that_exits_is_reported_with_its_exit_code()
    {
        using var client = new GpuWorkerClient(() => ProcessGpuWorker.Start(Cmd, "/d /c exit 3", _ => { }), Marker, Boot, Drivers);

        var e = Assert.Throws<GpuDriverCrashedException>(() => client.Adapters());

        Assert.Contains("crashed (exit code 0x00000003)", e.Message);
    }

    [Fact]
    public void A_real_hanging_worker_is_killed()
    {
        var pid = 0;
        // ping never reads its input and writes nothing to it for ~30 s.
        using var client = new GpuWorkerClient(
            () =>
            {
                var worker = ProcessGpuWorker.Start(Cmd, "/d /c ping -n 30 127.0.0.1 >nul", _ => { });
                pid = worker.Id;
                return worker;
            },
            Marker, Boot, Drivers, TimeSpan.FromSeconds(1), startTimeout: TimeSpan.FromSeconds(1));

        var e = Assert.Throws<GpuDriverCrashedException>(() => client.Adapters());

        Assert.Contains("did not answer within 1 s", e.Message);
        Assert.True(SpinWait.SpinUntil(() => !IsRunning(pid), TimeSpan.FromSeconds(5)), $"worker {pid} still running");
    }

    [Fact]
    public void A_worker_in_the_job_ends_the_moment_the_service_lets_go()
    {
        // The service's job handle closes when the service ends, however it
        // ends; closing it here stands in for that.
        var job = new KillOnCloseJob();
        using var worker = ProcessGpuWorker.Start(Cmd, "/d /c ping -n 30 127.0.0.1 >nul", _ => { });
        var process = System.Diagnostics.Process.GetProcessById(worker.Id);
        Assert.True(job.Add(process));
        Assert.False(worker.HasExited);

        job.Dispose();

        Assert.True(worker.WaitForExit(TimeSpan.FromSeconds(5)), "the worker outlived the job");
    }

    [Fact]
    public void The_shared_job_is_created()
    {
        Assert.NotNull(KillOnCloseJob.Shared);
    }

    [Fact]
    public void A_real_worker_forwards_its_log_lines_without_their_timestamp()
    {
        var lines = new ConcurrentQueue<string>();
        using var worker = ProcessGpuWorker.Start(Cmd, "/d /c echo [2026-10-15 19:00:00 +02:00] hello from the worker 1>&2", lines.Enqueue);

        Assert.True(worker.WaitForExit(TimeSpan.FromSeconds(5)));
        SpinWait.SpinUntil(() => !lines.IsEmpty, TimeSpan.FromSeconds(2));

        Assert.Contains("hello from the worker", lines); // no timestamp, no trailing space
    }

    private static bool IsRunning(int pid)
    {
        try
        {
            using var process = System.Diagnostics.Process.GetProcessById(pid);
            return !process.HasExited;
        }
        catch (ArgumentException)
        {
            return false; // no such process
        }
    }
}
