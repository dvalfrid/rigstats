using System.Text.RegularExpressions;
using SensorSidecar.Control.Lighting;
using SensorSidecar.Control.Mice;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Real replies from mice, headsets and keyboards (#572): the service log's
/// `Mouse: … read … from …`, `Headset: … read/equalizer … from …` and
/// `Keyboard: … read … from …` lines, copied verbatim from diagnostics ZIPs
/// into <c>fixtures/peripherals/*.log</c>. Each line's raw replies go through
/// today's parser and must read as the line says. A parser change that reads
/// a real device differently fails here; a deliberate one updates the line.
/// See fixtures/README.md, "Peripheral replies".
/// </summary>
public class PeripheralReplyFixtureTests
{
    private static string Root => Path.Combine(AppContext.BaseDirectory, "fixtures", "peripherals");

    private static readonly Regex Line = new(
        @"\] (?<kind>Mouse|Headset|Keyboard): (?<name>.+?) 0x(?<pid>[0-9A-F]{4})(?: \((?<link>\w+)\))? (?<what>read|equalizer) (?<read>.+) from (?<raw>[0-9A-F: ]+|nothing)\.$",
        RegexOptions.Compiled);

    public static IEnumerable<object[]> Lines()
    {
        if (!Directory.Exists(Root))
            yield break;
        foreach (var file in Directory.EnumerateFiles(Root, "*.log"))
        {
            var n = 0;
            foreach (var line in File.ReadLines(file))
            {
                n++;
                if (Line.IsMatch(line))
                    yield return [$"{Path.GetFileName(file)}:{n}", line];
            }
        }
    }

    [Fact]
    public void Every_fixture_line_is_a_reply_line()
    {
        if (!Directory.Exists(Root))
            return;
        foreach (var file in Directory.EnumerateFiles(Root, "*.log"))
            foreach (var line in File.ReadLines(file).Where(l => l.Trim().Length > 0))
                Assert.True(Line.IsMatch(line), $"{Path.GetFileName(file)}: not a reply line: {line}");
    }

    [Theory]
    [MemberData(nameof(Lines))]
    public void Reads_as_logged(string where, string line)
    {
        var m = Line.Match(line);
        var pid = Convert.ToUInt16(m.Groups["pid"].Value, 16);
        var read = m.Groups["read"].Value;
        var raw = m.Groups["raw"].Value;
        var actual = (m.Groups["kind"].Value, m.Groups["what"].Value) switch
        {
            ("Mouse", _) => Mouse(pid, m.Groups["link"].Value, raw, read),
            ("Headset", "equalizer") => Equalizer(pid, raw),
            ("Headset", _) => HeadsetSettingsRead(pid, raw),
            ("Keyboard", _) => Keyboard(raw),
            _ => throw new InvalidOperationException(line),
        };
        Assert.True(read == actual, $"{where}: logged '{read}', read now as '{actual}'");
    }

    // The log trims the zero padding (`Hid.ReplyHex`); the parsers want a
    // whole report.
    private static byte[] Reply(string hex)
    {
        var bytes = Convert.FromHexString(hex);
        return bytes.Length >= 64 ? bytes : [.. bytes, .. new byte[64 - bytes.Length]];
    }

    // `120000:<hex> 120400:<hex> …`, keyed as MouseProtocol.Parse asks. The
    // log doesn't say whether the Omni booster was on; it only changes the
    // polling rate's reading, so either reading that matches will do.
    private static string Mouse(ushort pid, string link, string raw, string logged)
    {
        var entry = MouseCatalog.Find(pid, Enum.Parse<MouseLink>(link, ignoreCase: true))
            ?? throw new InvalidOperationException($"0x{pid:X4} ({link}) is not in MouseCatalog");
        var replies = raw.Split(' ').Select(r => r.Split(':')).ToDictionary(p => p[0], p => Reply(p[1]));
        string Describe(bool booster) =>
            MouseProtocol.Parse(entry.Spec, replies, booster) is { } s ? AsusMouseDevice.Describe(s) : "nothing it could use";
        var plain = Describe(false);
        return plain == logged ? plain : Describe(true);
    }

    private static string HeadsetSettingsRead(ushort pid, string raw)
    {
        var limits = AsusHeadsetDevice.SettingsModels[pid];
        var r = raw.Split(' ').Select(Reply).ToArray();
        return HeadsetCommands.Parse(r[0], r[1], r[2], limits)?.ToString() ?? "nothing it could use";
    }

    private static string Equalizer(ushort pid, string raw)
    {
        if (raw == "nothing")
            return "not read";
        var reply = Reply(raw);
        var eq = HeadsetEqCommands.Parse(reply, reply[0], AsusHeadsetDevice.EqModels[pid]);
        return eq is null ? "not read" : $"{(eq.On ? "on" : "off")} {string.Join(",", eq.Gains)}";
    }

    private static string Keyboard(string raw)
    {
        var reply = Reply(raw);
        return KeyboardSettingsCommands.Parse(reply, reply[0]) is { } s
            ? $"idle {s.IdleCode}, power saving {s.PowerSaving}, low battery {s.LowBatteryLevel} %"
            : "nothing it could use";
    }
}
