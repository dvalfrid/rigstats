using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// #378: a background worker that throws must not stop the service — it is
/// logged and started again; only stopping the service ends it.
public class GuardedBackgroundServiceTests
{
    private sealed class FlakyWorker(int failures) : GuardedBackgroundService(TimeSpan.FromMilliseconds(10))
    {
        private int _runs;
        public int Runs => Volatile.Read(ref _runs);
        public TaskCompletionSource Running { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);

        protected override async Task RunAsync(CancellationToken stoppingToken)
        {
            if (Interlocked.Increment(ref _runs) <= failures)
                throw new IOException("The pipe is being closed.");
            Running.TrySetResult();
            await Task.Delay(Timeout.Infinite, stoppingToken);
        }
    }

    [Fact]
    public async Task A_worker_that_throws_is_started_again_and_keeps_running()
    {
        var worker = new FlakyWorker(failures: 3);
        await worker.StartAsync(CancellationToken.None);

        await worker.Running.Task.WaitAsync(TimeSpan.FromSeconds(5));

        Assert.Equal(4, worker.Runs);
        Assert.False(worker.ExecuteTask!.IsCompleted);
        await worker.StopAsync(CancellationToken.None);
        Assert.True(worker.ExecuteTask.IsCompletedSuccessfully);
    }

    [Fact]
    public async Task Stopping_during_the_pause_after_a_failure_ends_it_cleanly()
    {
        var worker = new FlakyWorker(failures: int.MaxValue);
        await worker.StartAsync(CancellationToken.None);
        // Stop only once it has failed and paused at least once: a fixed wait
        // could stop it before a starved CI thread pool started it at all.
        Assert.True(SpinWait.SpinUntil(() => worker.Runs > 1, TimeSpan.FromSeconds(10)), "the worker never ran twice");

        await worker.StopAsync(CancellationToken.None);

        Assert.True(worker.ExecuteTask!.IsCompletedSuccessfully);
        Assert.True(worker.Runs > 1);
    }
}
