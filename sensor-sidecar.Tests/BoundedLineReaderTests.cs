using System.Text;
using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// The control pipe's line reader (#378): lines as StreamReader reads them,
/// but never longer than the cap.
public class BoundedLineReaderTests
{
    private static BoundedLineReader Reader(string text, int max = 1024) =>
        new(new MemoryStream(Encoding.UTF8.GetBytes(text)), max);

    private static BoundedLineReader Reader(byte[] bytes, int max = 1024) => new(new MemoryStream(bytes), max);

    [Fact]
    public async Task Reads_lines_like_stream_reader()
    {
        var reader = Reader("{\"id\":1}\n{\"id\":2}\r\n\nlast");

        Assert.Equal("{\"id\":1}", await reader.ReadLineAsync(CancellationToken.None));
        Assert.Equal("{\"id\":2}", await reader.ReadLineAsync(CancellationToken.None));
        Assert.Equal("", await reader.ReadLineAsync(CancellationToken.None));
        Assert.Equal("last", await reader.ReadLineAsync(CancellationToken.None));
        Assert.Null(await reader.ReadLineAsync(CancellationToken.None));
    }

    [Fact]
    public async Task A_line_longer_than_the_buffer_is_read_whole_with_its_characters_intact()
    {
        // 3000 × "å" (two bytes each) crosses the 4 KB buffer mid-character.
        var text = new string('å', 3000);
        var reader = Reader(text + "\nnext\n", max: 10_000);

        Assert.Equal(text, await reader.ReadLineAsync(CancellationToken.None));
        Assert.Equal("next", await reader.ReadLineAsync(CancellationToken.None));
    }

    [Fact]
    public async Task A_line_past_the_cap_throws_before_it_is_all_buffered()
    {
        var reader = Reader(new byte[1_000_000], max: 64 * 1024);

        await Assert.ThrowsAsync<LineTooLongException>(() => reader.ReadLineAsync(CancellationToken.None));
    }

    [Fact]
    public async Task A_line_of_exactly_the_cap_is_fine()
    {
        var reader = Reader(new string('x', 100) + "\n", max: 100);

        Assert.Equal(100, (await reader.ReadLineAsync(CancellationToken.None))!.Length);
    }
}
