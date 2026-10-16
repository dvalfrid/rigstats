namespace SensorSidecar.Control.Workers;

/// A running worker process as its client sees it (#370, #374): requests go in
/// on its input, answers come back on its output, one JSON line each. A seam
/// so clients are testable without a real process.
public interface IWorker : IDisposable
{
    TextWriter Input { get; }
    TextReader Output { get; }
    bool HasExited { get; }

    /// The exit code once it has exited, else null.
    int? ExitCode { get; }

    bool WaitForExit(TimeSpan timeout);
    void Kill();
}
