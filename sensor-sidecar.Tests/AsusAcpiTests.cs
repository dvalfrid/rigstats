using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// ASUS ATKACPI (#234), read-only step: the `DSTS` buffer and the presence
/// rule from docs/asus-laptop-acpi.md ("Buffer layout", "Return values").
/// </summary>
public sealed class AsusAcpiTests
{
    [Fact]
    public void Dsts_input_is_method_length_id_argument_little_endian()
    {
        Assert.Equal(
            Convert.FromHexString("44535453" + "08000000" + "75001200" + "02000000"),
            AsusAcpi.DstsInput(0x00120075, 2));
    }

    [Theory]
    [InlineData(0x00010000u, true)]   // present, value 0
    [InlineData(0x00010002u, true)]   // present, value 2
    [InlineData(0x00000000u, false)]  // not present
    [InlineData(0xFFFFFFFEu, false)]  // method unsupported
    [InlineData(0xFFFFFFFFu, false)]  // no device
    public void Presence_is_bit_16_except_the_error_words(uint raw, bool present) =>
        Assert.Equal(present, AsusAcpi.IsPresent(raw));

    [Fact]
    public void Only_the_documented_ids_are_read()
    {
        // Pinned so an id can't slip in unreviewed; APU memory (0x000600C1)
        // and GPU Eco/MUX are never on the list.
        Assert.Equal(
            new uint[]
            {
                0x00120075, 0x00110019, 0x00110013, 0x00110014, 0x00110031,
                0x001200A3, 0x001200A0, 0x001200C1, 0x001200B0, 0x001200B1,
                0x001200C0, 0x001200C2, 0x00120099, 0x00120098,
            },
            AsusAcpi.ReadIds);
        Assert.Equal(new uint[] { 0x00110024, 0x00110025, 0x00110032 }, AsusAcpi.CurveIds);
    }

    public static TheoryData<string> Fixtures()
    {
        var data = new TheoryData<string>();
        var dir = Path.Combine(AppContext.BaseDirectory, "fixtures", "asus-acpi");
        foreach (var file in Directory.GetFiles(dir, "*.log").Order())
            data.Add(Path.GetFileName(file));
        return data;
    }

    /// Real logs from laptops (fixtures/asus-acpi/): every id on the list is
    /// in each snapshot, every word that answered is marked absent exactly
    /// when the presence rule says so, and each curve is 16 bytes or a fail.
    [Theory]
    [MemberData(nameof(Fixtures))]
    public void Logged_snapshots_follow_the_presence_rule(string name)
    {
        var lines = File.ReadLines(Path.Combine(AppContext.BaseDirectory, "fixtures", "asus-acpi", name))
            .Where(l => l.StartsWith("[rigstats-control] ASUS ACPI", StringComparison.Ordinal))
            .ToList();

        var snapshots = lines.Where(l => l.StartsWith("[rigstats-control] ASUS ACPI DSTS: ", StringComparison.Ordinal)).ToList();
        Assert.NotEmpty(snapshots);
        foreach (var line in snapshots)
        {
            var words = line["[rigstats-control] ASUS ACPI DSTS: ".Length..].Split(' ')
                .Select(w => w.Split('='))
                .ToDictionary(p => Convert.ToUInt32(p[0], 16), p => p[1]);
            Assert.Equal(AsusAcpi.ReadIds.Order(), words.Keys.Order());
            foreach (var word in words.Values.Where(w => w != "fail"))
            {
                var absent = word.EndsWith("(absent)", StringComparison.Ordinal);
                var raw = Convert.ToUInt32(word.Replace("(absent)", "", StringComparison.Ordinal), 16);
                Assert.Equal(!absent, AsusAcpi.IsPresent(raw));
            }
        }

        var curves = lines.Where(l => l.StartsWith("[rigstats-control] ASUS ACPI curve ", StringComparison.Ordinal)).ToList();
        Assert.Equal(AsusAcpi.CurveIds.Order(),
            curves.Select(l => Convert.ToUInt32(l.Split(' ')[4], 16)).Order());
        foreach (var line in curves)
        {
            var modes = line[(line.IndexOf("):", StringComparison.Ordinal) + 3)..].Split(" [").Select(m => m.Split("] ")[1]).ToList();
            Assert.Equal(3, modes.Count);
            Assert.All(modes, m => Assert.True(m == "fail" || m.Length == 32, m));
        }
    }
}
