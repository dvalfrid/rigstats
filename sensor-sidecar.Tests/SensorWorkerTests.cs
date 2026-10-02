using System.IO.Pipes;
using LibreHardwareMonitor.Hardware;
using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// The multi-client telemetry pipe (#196, hardened in #204), driven over a
/// real named pipe with a per-test name so it never collides with an
/// installed <c>rigstats-sensor</c> service. Clients get a line about once a
/// second, so these tests take a few seconds each.
/// </summary>
public class SensorWorkerTests
{
    private static readonly TimeSpan ReadTimeout = TimeSpan.FromSeconds(5);

    /// Counts telemetry requests; can fail the next one on demand.
    private sealed class FakeTelemetryHost : IHardwareHost
    {
        private int _calls;
        private int _failNext;

        public int Calls => Volatile.Read(ref _calls);

        public void FailNext() => Volatile.Write(ref _failNext, 1);

        /// When set, the next request blocks — ignoring cancellation, like an
        /// LHM read in `RefreshIfStale` — until `Release` is set.
        public TaskCompletionSource? BlockNext { get; set; }
        public TaskCompletionSource Entered { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);

        public async Task<string> GetTelemetryLineAsync(CancellationToken ct)
        {
            ct.ThrowIfCancellationRequested();
            var n = Interlocked.Increment(ref _calls);
            if (Interlocked.Exchange(ref _failNext, 0) == 1)
                throw new InvalidOperationException("LHM failed");
            if (BlockNext is { } gate)
            {
                BlockNext = null;
                Entered.TrySetResult();
                await gate.Task;
            }
            return $"{{\"n\":{n}}}";
        }

        public Task<SensorPayload> GetSampleAsync(CancellationToken ct) => throw new NotSupportedException();

        public Task<T> WithHardwareLockAsync<T>(Func<IComputer, T> action, CancellationToken ct) =>
            throw new NotSupportedException();

        public void WriteSensorTree(string path) { }
    }

    private sealed class Client(NamedPipeClientStream pipe) : IDisposable
    {
        private readonly StreamReader _reader = new(pipe);

        public async Task<string?> ReadLineAsync()
        {
            using var cts = new CancellationTokenSource(ReadTimeout);
            return await _reader.ReadLineAsync(cts.Token);
        }

        /// Reads until EOF, skipping lines buffered before the server closed.
        public async Task<bool> SeesEofAsync()
        {
            for (var i = 0; i < 5; i++)
            {
                if (await ReadLineAsync() is null)
                    return true;
            }
            return false;
        }

        public void Dispose() => pipe.Dispose();
    }

    private static string NewPipeName() => $"rigstats-sensors-test-{Guid.NewGuid():N}";

    private static async Task<Client> ConnectAsync(string pipeName, TimeSpan? timeout = null)
    {
        var pipe = new NamedPipeClientStream(".", pipeName, PipeDirection.In, PipeOptions.Asynchronous);
        try
        {
            await pipe.ConnectAsync((int)(timeout ?? ReadTimeout).TotalMilliseconds);
        }
        catch
        {
            pipe.Dispose();
            throw;
        }
        return new Client(pipe);
    }

    private static async Task<SensorWorker> StartAsync(FakeTelemetryHost host, string pipeName)
    {
        var worker = new SensorWorker(host, pipeName);
        await worker.StartAsync(CancellationToken.None);
        return worker;
    }

    private static async Task StopAsync(SensorWorker worker)
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        await worker.StopAsync(cts.Token);
    }

    [Fact]
    public async Task Max_clients_plus_more_attempts_keeps_the_service_running()
    {
        var host = new FakeTelemetryHost();
        var name = NewPipeName();
        var worker = await StartAsync(host, name);
        var clients = new List<Client>();
        try
        {
            for (var i = 0; i < 4; i++)
            {
                clients.Add(await ConnectAsync(name));
                Assert.NotNull(await clients[i].ReadLineAsync());
            }

            // Every instance is in use: the extra client can't connect, and
            // creating the next instance must not have faulted the worker.
            await Assert.ThrowsAsync<TimeoutException>(() => ConnectAsync(name, TimeSpan.FromSeconds(1)));
            Assert.False(worker.ExecuteTask!.IsCompleted);
            foreach (var c in clients)
                Assert.NotNull(await c.ReadLineAsync());

            // A freed slot is offered again (within the 2 s retry back-off).
            clients[0].Dispose();
            clients.RemoveAt(0);
            clients.Add(await ConnectAsync(name));
            Assert.NotNull(await clients[^1].ReadLineAsync());
        }
        finally
        {
            clients.ForEach(c => c.Dispose());
            await StopAsync(worker);
        }
    }

    [Fact]
    public async Task Stop_with_clients_connected_ends_them_and_stops_sampling()
    {
        var host = new FakeTelemetryHost();
        var name = NewPipeName();
        var worker = await StartAsync(host, name);
        using var a = await ConnectAsync(name);
        using var b = await ConnectAsync(name);
        Assert.NotNull(await a.ReadLineAsync());
        Assert.NotNull(await b.ReadLineAsync());

        await StopAsync(worker);

        // Once StopAsync returns no client may still be sampling — that is
        // what lets HardwareHost close the Computer safely.
        var callsAtStop = host.Calls;
        Assert.True(await a.SeesEofAsync());
        Assert.True(await b.SeesEofAsync());
        await Task.Delay(1500);
        Assert.Equal(callsAtStop, host.Calls);
    }

    [Fact]
    public async Task Stop_waits_for_a_client_that_is_mid_sample()
    {
        var host = new FakeTelemetryHost();
        var name = NewPipeName();
        var worker = await StartAsync(host, name);
        using var a = await ConnectAsync(name);
        Assert.NotNull(await a.ReadLineAsync());

        var gate = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        host.BlockNext = gate;
        await host.Entered.Task.WaitAsync(ReadTimeout);

        // HardwareHost stops right after this worker — returning while a
        // client is still sampling is the #204 race.
        var stop = StopAsync(worker);
        await Task.Delay(300);
        Assert.False(stop.IsCompleted);

        gate.SetResult();
        await stop;
    }

    [Fact]
    public async Task A_client_that_stops_reading_neither_stalls_others_nor_blocks_stop()
    {
        var host = new FakeTelemetryHost();
        var name = NewPipeName();
        var worker = await StartAsync(host, name);
        using var hung = await ConnectAsync(name); // never read
        using var live = await ConnectAsync(name);

        for (var i = 0; i < 3; i++)
            Assert.NotNull(await live.ReadLineAsync());

        var stop = Task.Run(() => StopAsync(worker));
        Assert.Same(stop, await Task.WhenAny(stop, Task.Delay(TimeSpan.FromSeconds(5))));
        await stop;
    }

    [Fact]
    public async Task A_client_disconnecting_mid_stream_leaves_the_others_running()
    {
        var host = new FakeTelemetryHost();
        var name = NewPipeName();
        var worker = await StartAsync(host, name);
        try
        {
            using var a = await ConnectAsync(name);
            using var b = await ConnectAsync(name);
            Assert.NotNull(await a.ReadLineAsync());
            a.Dispose();

            for (var i = 0; i < 3; i++)
                Assert.NotNull(await b.ReadLineAsync());
            Assert.False(worker.ExecuteTask!.IsCompleted);
        }
        finally
        {
            await StopAsync(worker);
        }
    }

    [Fact]
    public async Task A_sampling_failure_ends_only_that_client()
    {
        var host = new FakeTelemetryHost();
        var name = NewPipeName();
        var worker = await StartAsync(host, name);
        try
        {
            using var a = await ConnectAsync(name);
            Assert.NotNull(await a.ReadLineAsync());

            host.FailNext();
            Assert.True(await a.SeesEofAsync());

            using var b = await ConnectAsync(name);
            Assert.NotNull(await b.ReadLineAsync());
            Assert.NotNull(await b.ReadLineAsync());
            Assert.False(worker.ExecuteTask!.IsCompleted);
        }
        finally
        {
            await StopAsync(worker);
        }
    }
}
