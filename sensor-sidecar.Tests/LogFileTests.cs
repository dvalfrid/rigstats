using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// #223: the service log stays bounded while the service runs, not only
/// across restarts — and truncating never loses the newest lines.
/// </summary>
public sealed class LogFileTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), $"rigstats-logfile-{Guid.NewGuid():N}");
    private string LogPath => Path.Combine(_dir, "test.log");

    public void Dispose()
    {
        if (Directory.Exists(_dir))
            Directory.Delete(_dir, recursive: true);
    }

    [Fact]
    public void AppendingPastTheLimitInOneRunKeepsTheFileBoundedAndEndingWithTheNewestLine()
    {
        var log = new LogFile(LogPath, maxBytes: 4096, keepLines: 20, checkEveryBytes: 512);

        for (var i = 0; i < 2000; i++)
            log.Append($"line {i:D5}");

        Assert.True(new FileInfo(LogPath).Length <= 4096 + 512, $"log is {new FileInfo(LogPath).Length} bytes");
        Assert.Equal("line 01999", File.ReadLines(LogPath).Last());
        Assert.False(File.Exists(LogPath + ".tmp"));
    }

    [Fact]
    public void TruncateIfNeededLeavesASmallLogAlone()
    {
        var log = new LogFile(LogPath, maxBytes: 4096, keepLines: 2, checkEveryBytes: 512);
        for (var i = 0; i < 10; i++)
            log.Append($"line {i}");

        log.TruncateIfNeeded();

        Assert.Equal(10, File.ReadAllLines(LogPath).Length);
    }

    [Fact]
    public void TruncateIfNeededCutsALargeLogToItsLastLines()
    {
        var log = new LogFile(LogPath, maxBytes: 100, keepLines: 3, checkEveryBytes: long.MaxValue);
        for (var i = 0; i < 50; i++)
            log.Append($"line {i}");

        log.TruncateIfNeeded();

        Assert.Equal(["line 47", "line 48", "line 49"], File.ReadAllLines(LogPath));
    }

    [Fact]
    public async Task ConcurrentAppendsAcrossTruncatesLoseNoLineWrittenAfterThem()
    {
        var log = new LogFile(LogPath, maxBytes: 4096, keepLines: 50, checkEveryBytes: 512);
        const int writers = 8, perWriter = 500;

        await Task.WhenAll(Enumerable.Range(0, writers).Select(w => Task.Run(() =>
        {
            for (var i = 0; i < perWriter; i++)
                log.Append($"w{w} {i:D3}");
        })));
        log.Append("last");

        var lines = File.ReadAllLines(LogPath);
        Assert.Equal("last", lines[^1]);
        // Truncating keeps the newest lines in write order, so what is left of
        // each writer is an unbroken run ending with its last line.
        for (var w = 0; w < writers; w++)
        {
            var kept = lines.Where(l => l.StartsWith($"w{w} ")).Select(l => int.Parse(l[(l.IndexOf(' ') + 1)..])).ToList();
            if (kept.Count == 0)
                continue;
            Assert.Equal(Enumerable.Range(perWriter - kept.Count, kept.Count), kept);
        }
    }
}
