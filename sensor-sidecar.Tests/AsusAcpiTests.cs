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
}
