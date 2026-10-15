using Microsoft.Extensions.Hosting;

namespace SensorSidecar.Control;

/// A background worker that can't stop the service (#378). An exception out of
/// `BackgroundService.ExecuteAsync` stops the whole host (the default
/// `BackgroundServiceExceptionBehavior` is `StopHost`) — telemetry, fan curves
/// and lighting with it, for one worker's trouble. Every worker of the service
/// derives from this instead: a failure is logged and the work starts again
/// after a short pause; only stopping the service ends it. A worker still
/// handles the failures it expects itself — this is the backstop.
public abstract class GuardedBackgroundService : BackgroundService
{
    internal static readonly TimeSpan DefaultRetryDelay = TimeSpan.FromSeconds(2);

    private readonly TimeSpan _retryDelay;
    private readonly RepeatLog _failures;

    /// `retryDelay` is only shortened by tests.
    protected GuardedBackgroundService(TimeSpan? retryDelay = null)
    {
        _retryDelay = retryDelay ?? DefaultRetryDelay;
        _failures = new RepeatLog($"{LogTag} {GetType().Name} failed and starts again", "failed");
    }

    /// The log line prefix, e.g. "[rigstats-sensor]".
    protected virtual string LogTag => "[rigstats-control]";

    /// The worker's loop. Returning ends the worker; throwing starts it again.
    protected abstract Task RunAsync(CancellationToken stoppingToken);

    protected sealed override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                await RunAsync(stoppingToken);
                return;
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                return;
            }
            catch (Exception e)
            {
                // The stack the first time; a failure that repeats every pause
                // is logged once per reason (with its count), not every 2 s.
                if (_failures.Line(e.GetType().Name + ": " + e.Message, DateTimeOffset.Now) is { } line)
                    SidecarLog.Log($"{line}{Environment.NewLine}{e}");
            }
            try
            {
                await Task.Delay(_retryDelay, stoppingToken);
            }
            catch (OperationCanceledException)
            {
                return;
            }
        }
    }
}
