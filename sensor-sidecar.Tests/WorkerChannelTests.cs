using SensorSidecar.Control.Workers;
using Xunit;

namespace SensorSidecar.Tests;

/// #374: the shared worker plumbing — answers in order, a hang is told apart
/// from a crash, and a broken pipe is the caller's to see.
public class WorkerChannelTests
{
    /// Output the test writes; input recorded.
    private sealed class PipeWorker : IWorker
    {
        private readonly AnonymousPipes _out = new();
        public StringWriter Sent { get; } = new();
        public TextWriter Input => Sent;
        public TextReader Output => _out.Reader;
        public StreamWriter Answers => _out.Writer;
        public bool HasExited { get; set; }
        public int? ExitCode => HasExited ? 0 : null;
        public bool Killed { get; private set; }
        public bool WaitForExit(TimeSpan timeout) => HasExited;
        public void Kill() => Killed = true;
        public void Dispose() => _out.Dispose();
    }

    /// A real in-process pipe, so the reader thread blocks like on a process.
    private sealed class AnonymousPipes : IDisposable
    {
        private readonly System.IO.Pipes.AnonymousPipeServerStream _server = new(System.IO.Pipes.PipeDirection.Out);
        private readonly System.IO.Pipes.AnonymousPipeClientStream _client;

        public AnonymousPipes()
        {
            _client = new(System.IO.Pipes.PipeDirection.In, _server.ClientSafePipeHandle);
            Writer = new StreamWriter(_server) { AutoFlush = true };
            Reader = new StreamReader(_client);
        }

        public StreamWriter Writer { get; }
        public StreamReader Reader { get; }

        public void Dispose()
        {
            Writer.Dispose();
            Reader.Dispose();
        }
    }

    [Fact]
    public void Answers_arrive_in_order_and_requests_go_out_as_lines()
    {
        var worker = new PipeWorker();
        using var channel = new WorkerChannel(worker, "test-output");

        channel.Send("""{"id":1}""");
        worker.Answers.WriteLine("one");
        worker.Answers.WriteLine("two");

        Assert.Equal((true, "one"), channel.Receive(TimeSpan.FromSeconds(5)));
        Assert.Equal((true, "two"), channel.Receive(TimeSpan.FromSeconds(5)));
        Assert.Equal("{\"id\":1}" + Environment.NewLine, worker.Sent.ToString());
    }

    [Fact]
    public void Silence_is_a_hang_not_an_end()
    {
        using var channel = new WorkerChannel(new PipeWorker(), "test-output");

        Assert.Equal((false, null), channel.Receive(TimeSpan.FromMilliseconds(200)));
    }

    [Fact]
    public void Output_that_ends_is_an_answer_of_null()
    {
        var worker = new PipeWorker();
        using var channel = new WorkerChannel(worker, "test-output");

        worker.Answers.Dispose(); // the worker exited

        Assert.Equal((true, null), channel.Receive(TimeSpan.FromSeconds(5)));
    }

    [Fact]
    public void Kill_never_throws()
    {
        var worker = new PipeWorker();
        using var channel = new WorkerChannel(worker, "test-output");

        channel.Kill();

        Assert.True(worker.Killed);
    }
}
