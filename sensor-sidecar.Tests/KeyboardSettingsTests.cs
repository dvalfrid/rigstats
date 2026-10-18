using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// A keyboard's energy settings (#460): the <c>12 01</c> reply's fields and
/// the <c>51 38</c> / <c>51 39</c> / <c>51 3A</c> frames, against Gear
/// Link's keyboard power class and the ROG Azoth X's live reply.
/// </summary>
public class KeyboardSettingsTests
{
    // The Azoth X's 12 01 reply through the Omni receiver (report 2), read
    // live on 2026-10-18 (#457): 67 %, idle 3 min, lighting off, not
    // charging, low battery 20 %.
    private static readonly byte[] AzothX = Convert.FromHexString("0212010000004302010014436F0F00000000");

    [Fact]
    public void The_power_reply_gives_idle_power_saving_and_the_low_battery_level()
    {
        Assert.Equal(new KeyboardSettings(2, 1, 20), KeyboardSettingsCommands.Parse(AzothX, 0x02));
        Assert.Null(KeyboardSettingsCommands.Parse(AzothX, 0x00)); // another report id
        Assert.Null(KeyboardSettingsCommands.Parse(Convert.FromHexString("02120000000800070006000124000100"), 0x02)); // 12 00
        Assert.Null(KeyboardSettingsCommands.Parse(Convert.FromHexString("0212010000004307010014"), 0x02)); // idle code 7: not one of the six
        Assert.Null(KeyboardSettingsCommands.Parse(null, 0x02));
    }

    [Fact]
    public void Only_what_differs_is_written_with_its_own_key()
    {
        var now = new KeyboardSettings(2, 1, 20);

        Assert.Empty(KeyboardSettingsCommands.Writes(0x02, 64, now, now));
        var writes = KeyboardSettingsCommands.Writes(0x02, 6, now, new KeyboardSettings(255, 2, 25));
        Assert.Equal(["0251380000FF", "025139000002", "02513A000019"], writes.Select(Convert.ToHexString));
        Assert.Equal(["025138000004"], KeyboardSettingsCommands.Writes(0x02, 6, now, now with { IdleCode = 4 }).Select(Convert.ToHexString));
        Assert.Equal(64, KeyboardSettingsCommands.Writes(0x02, 64, now, now with { PowerSaving = 0 })[0].Length);
        // 51 37 is the mouse's power frame in Gear Link — never a keyboard's.
        Assert.DoesNotContain(writes, w => w[2] == 0x37);
    }

    [Fact]
    public void Values_are_kept_inside_the_keyboards_ranges()
    {
        Assert.Equal(new KeyboardSettings(255, 2, 100), KeyboardSettingsCommands.Clamp(new KeyboardSettings(7, 9, 140)));
        Assert.Equal(new KeyboardSettings(3, 0, 0), KeyboardSettingsCommands.Clamp(new KeyboardSettings(3, -1, -5)));
    }
}
