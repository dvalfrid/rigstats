using Microsoft.Extensions.Hosting;

namespace SensorSidecar.Control;

/// One-off work at start that nothing else waits for — reading earlier crashes
/// from the event log (#242). It runs in the background once the service has
/// started (#383): before `host.Run()` it would count against the 30 s the
/// service manager gives a starting service, and in a hosted service's
/// `StartAsync` it would hold up telemetry. A failure is logged, never thrown;
/// stopping the service waits for it (or the host's stop timeout). Register
/// it once: `AddHostedService` keeps only the first registration of a type
/// (`TryAddEnumerable`), so a second task of this class would silently never
/// run (seen live, #193) — put more work into the one task instead.
public sealed class BackgroundStartupTask(string name, Action work) : IHostedService
{
    private Task? _running;

    public Task StartAsync(CancellationToken cancellationToken)
    {
        _running = Task.Run(() =>
        {
            try
            {
                work();
            }
            catch (Exception e)
            {
                SidecarLog.Log($"[rigstats-sensor] {name} failed: {e.Message}");
            }
        }, CancellationToken.None);
        return Task.CompletedTask;
    }

    public async Task StopAsync(CancellationToken cancellationToken)
    {
        if (_running is { } running)
        {
            try
            {
                await running.WaitAsync(cancellationToken);
            }
            catch (OperationCanceledException)
            {
                // The host's stop timeout: the work is abandoned with the process.
            }
        }
    }
}
