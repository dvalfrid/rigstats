using System.Collections.Concurrent;

namespace SensorSidecar.Control.Workers;

/// One running worker and its output, read line by line on a thread of its own
/// (#370, #374) — a call waits here with its own time limit, so nothing
/// depends on the thread pool, which may be what is starved. What a client
/// does when the worker crashes or hangs (give up until a reboot, restart at
/// once) is the client's policy; this only tells it which happened.
public sealed class WorkerChannel : IDisposable
{
    private readonly BlockingCollection<string?> _lines = new();

    /// `name` names the reader thread ("gpu-worker-output").
    public WorkerChannel(IWorker worker, string name)
    {
        Worker = worker;
        var reader = new Thread(() =>
        {
            try
            {
                while (worker.Output.ReadLine() is { } line)
                    _lines.Add(line);
            }
            catch
            {
                // The stream broke: the worker is gone.
            }
            finally
            {
                // Null marks the end: the worker closed its output or exited.
                _lines.Add(null);
            }
        })
        {
            IsBackground = true,
            Name = name,
        };
        reader.Start();
    }

    public IWorker Worker { get; }

    /// Sends one line. Throws `IOException` when the pipe broke (the worker
    /// is gone).
    public void Send(string line)
    {
        Worker.Input.WriteLine(line);
        Worker.Input.Flush();
    }

    /// The next line, or null when the worker's output ended. `Answered` is
    /// false when nothing came within `timeout` — a hang, not a crash.
    public (bool Answered, string? Line) Receive(TimeSpan timeout) =>
        _lines.TryTake(out var line, timeout) ? (true, line) : (false, null);

    /// Ends the worker; never throws.
    public void Kill()
    {
        try
        {
            Worker.Kill();
        }
        catch
        {
            // Already gone.
        }
    }

    /// Ends the worker. The line queue is left alone: the reader thread may
    /// still add its end marker, and an exception there would end the service.
    public void Dispose() => Worker.Dispose();
}
