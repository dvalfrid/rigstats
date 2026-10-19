using System.Text.Json.Nodes;
using SensorSidecar.Control;
using SensorSidecar.Control.Keyboards;
using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// A keyboard's chosen energy settings survive its onboard-profile switches
/// (#497): it loads its own saved ones on a switch, so RIGStats keeps the
/// chosen ones and writes them again.
/// </summary>
public class KeyboardEnergyTests
{
    [Fact]
    public void The_store_keeps_each_keyboards_choice_across_a_restart()
    {
        var path = Path.Combine(Path.GetTempPath(), $"keyboard-energy-{Guid.NewGuid():N}.json");
        try
        {
            new KeyboardEnergyStore(path).Set("asus-keyboard-1ace-1", new KeyboardSettings(3, 1, 20));

            var again = new KeyboardEnergyStore(path);
            Assert.Equal(new KeyboardSettings(3, 1, 20), again.Get("asus-keyboard-1ace-1"));
            Assert.Null(again.Get("asus-keyboard-1b7e-1"));
        }
        finally
        {
            File.Delete(path);
        }
    }

    [Fact]
    public void A_keyboard_that_lost_its_settings_gets_them_again_and_one_that_has_them_isnt_written()
    {
        var path = Path.Combine(Path.GetTempPath(), $"keyboard-energy-{Guid.NewGuid():N}.json");
        try
        {
            var store = new KeyboardEnergyStore(path);
            var keyboard = new FakeKeyboard();
            var provider = new LightingProvider([keyboard], "", () => null, dryRun: false) { KeyboardEnergy = store };

            provider.SetKeyboard(keyboard.Id, new KeyboardSettings(3, 1, 20)); // 5 minutes, chosen
            keyboard.Loaded(new KeyboardSettings(2, 1, 20));                   // a switch put 3 minutes back
            provider.KeepKeyboardEnergy();

            Assert.Equal(new KeyboardSettings(3, 1, 20), keyboard.Settings);
            Assert.Equal(2, keyboard.Writes);

            provider.KeepKeyboardEnergy(); // already right: only read
            Assert.Equal(2, keyboard.Writes);
        }
        finally
        {
            File.Delete(path);
        }
    }

    [Fact]
    public void A_profile_switch_puts_the_chosen_energy_settings_back()
    {
        var kept = 0;
        var provider = new KeyboardProvider(() => [new FakeKeyboard()], dryRun: false, () => kept++);

        provider.Apply(new ProfilePart { Keyboard = new KeyboardPart { Profile = 2 } });

        Assert.Equal(1, kept);
    }

    private sealed class FakeKeyboard : ILightingDevice, IKeyboardProfileDevice
    {
        public string Id => "asus-keyboard-1ace-1";
        public string Name => "ROG Azoth X";
        public string Kind => "keyboard";
        public string Firmware => "";
        public string? Blocked => null;
        public IReadOnlyList<AuraZone> Zones { get; } = [new("keys", "Keys", true, 0, 1)];
        public void Apply(AuraEffect effect, byte red, byte green, byte blue) { }
        public bool Release() => false;
        public JsonObject Diagnostics() => [];

        public ushort ProductId => 0x1ACE;
        public KeyboardProfiles? FoundProfiles => new(6, 1);
        public KeyboardProfiles? ReadProfiles() => new(6, 1);
        public void SwitchProfile(int profile) { }
        public bool HasBattery => true;
        public bool SettingsVerified => true;

        public KeyboardSettings Settings { get; private set; } = new(2, 1, 20);
        public int Writes { get; private set; }

        /// The keyboard loading its own saved settings (an onboard-profile switch).
        public void Loaded(KeyboardSettings settings) => Settings = settings;

        public KeyboardSettings? ReadSettings() => Settings;

        public KeyboardSettings WriteSettings(KeyboardSettings wanted)
        {
            Writes++;
            return Settings = wanted;
        }
    }
}
