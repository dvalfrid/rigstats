using Microsoft.Extensions.Logging;

namespace SensorSidecar.Control;

/// The host's own warnings and errors in `rigstats-sensor.log` (#378): a
/// background worker that failed, the host stopping because of one, a hosted
/// service that didn't stop in time. Otherwise they only reach the Windows
/// event log, which a diagnostics export doesn't carry.
public sealed class SidecarLoggerProvider : ILoggerProvider
{
    public ILogger CreateLogger(string categoryName) => new Logger(categoryName);

    public void Dispose() { }

    private sealed class Logger(string category) : ILogger
    {
        public IDisposable? BeginScope<TState>(TState state) where TState : notnull => null;

        public bool IsEnabled(LogLevel logLevel) => logLevel >= LogLevel.Warning;

        public void Log<TState>(LogLevel logLevel, EventId eventId, TState state, Exception? exception,
            Func<TState, Exception?, string> formatter)
        {
            if (!IsEnabled(logLevel))
                return;
            var message = formatter(state, exception);
            SidecarLog.Log($"[host] {logLevel} {category}: {message}{(exception is null ? "" : Environment.NewLine + exception)}");
        }
    }
}
