using System.Text;

namespace SensorSidecar.Control;

/// A line longer than the reader allows (#378).
public sealed class LineTooLongException(int maxBytes)
    : IOException($"a line longer than {maxBytes / 1024} KB");

/// Reads newline-delimited UTF-8 lines from a stream, each at most `maxBytes`
/// (#378). `StreamReader.ReadLineAsync` buffers until it sees a newline,
/// however long that takes — on the control pipe a client could make the
/// SYSTEM service hold any amount of memory. The app caps a frame the same way
/// (`MAX_PIPE_LINE_BYTES` in `rigstats-backend`). Lines are decoded whole, so
/// a character split across two reads is never cut.
[System.Diagnostics.CodeAnalysis.SuppressMessage("Design", "CA1001", Justification = "A MemoryStream holds only memory; the reader lives as long as its connection.")]
public sealed class BoundedLineReader(Stream stream, int maxBytes)
{
    private readonly byte[] _buffer = new byte[4096];
    private readonly MemoryStream _line = new();
    private int _start;
    private int _end;

    /// The next line without its `\n` (or `\r\n`), the last one even without
    /// a newline, or null at the end of the stream. Throws
    /// <see cref="LineTooLongException"/> once a line passes `maxBytes`.
    public async Task<string?> ReadLineAsync(CancellationToken ct)
    {
        _line.SetLength(0);
        while (true)
        {
            if (_start == _end)
            {
                _start = 0;
                _end = await stream.ReadAsync(_buffer, ct);
                if (_end == 0)
                    return _line.Length > 0 ? Decode() : null;
            }
            var newline = Array.IndexOf(_buffer, (byte)'\n', _start, _end - _start);
            var take = (newline < 0 ? _end : newline) - _start;
            if (_line.Length + take > maxBytes)
                throw new LineTooLongException(maxBytes);
            _line.Write(_buffer, _start, take);
            if (newline < 0)
            {
                _start = _end;
                continue;
            }
            _start = newline + 1;
            return Decode();
        }
    }

    private string Decode()
    {
        var bytes = _line.GetBuffer().AsSpan(0, (int)_line.Length);
        if (bytes.Length > 0 && bytes[^1] == (byte)'\r')
            bytes = bytes[..^1];
        return Encoding.UTF8.GetString(bytes);
    }
}
