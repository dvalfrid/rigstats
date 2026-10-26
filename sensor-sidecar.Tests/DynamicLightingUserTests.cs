using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <see cref="LampArrayDevice.DynamicLightingOn"/>: whose Windows Dynamic
/// Lighting setting decides whether RIGStats leaves LampArray devices alone
/// (#636).
public class DynamicLightingUserTests
{
    private const string Owner = "S-1-5-21-1-2-3-1001";
    private const string Guest = "S-1-5-21-1-2-3-1002";

    [Fact]
    public void A_guest_signed_in_in_the_background_does_not_block_the_user_at_the_screen()
    {
        // The G14: the guest has it on, the owner (active) off.
        Assert.False(LampArrayDevice.DynamicLightingOn(Owner, [(Owner, false), (Guest, true)]));
    }

    [Fact]
    public void The_user_at_the_screen_with_it_on_blocks()
    {
        Assert.True(LampArrayDevice.DynamicLightingOn(Guest, [(Owner, false), (Guest, true)]));
    }

    [Fact]
    public void A_user_without_the_value_has_it_off()
    {
        Assert.False(LampArrayDevice.DynamicLightingOn(Owner, [(Owner, null), (Guest, true)]));
    }

    [Fact]
    public void Without_a_known_active_user_any_signed_in_user_with_it_on_counts()
    {
        // The sign-in screen, or the active user's hive isn't loaded: yield.
        Assert.True(LampArrayDevice.DynamicLightingOn(null, [(Owner, false), (Guest, true)]));
        Assert.True(LampArrayDevice.DynamicLightingOn("S-1-5-21-9-9-9-1000", [(Guest, true)]));
        Assert.False(LampArrayDevice.DynamicLightingOn(null, [(Owner, false)]));
    }
}
