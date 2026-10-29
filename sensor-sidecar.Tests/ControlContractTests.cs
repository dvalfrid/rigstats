using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using SensorSidecar.Control;
using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// The control pipe's answers and the profile, as the shared examples in
/// <c>contract/</c> that the app's reader parses too (<c>control.rs</c>,
/// <c>contract_examples_round_trip</c>) — every field filled in, so a field
/// renamed, added or dropped on one side fails one of the two (#679). The
/// mouse's is in <c>MouseProtocolTests</c>, next to its fake.
/// </summary>
public class ControlContractTests
{
    /// Every part of a profile, as `list_profiles` sends it and
    /// `save_profile` / `preview` take it.
    private static Profile FullProfile() => new()
    {
        Id = "my-rig",
        Name = "My rig",
        Icon = "bolt",
        Builtin = false,
        Part = new ProfilePart
        {
            PowerPlan = "high_performance",
            Fan = new FanPart
            {
                Headers = new()
                {
                    ["lpc/nct6799d/0/control/1"] = new FanHeaderConfig
                    {
                        Label = "CPU cooler",
                        Source = "cpu_package",
                        Curve = [[30, 25], [60, 50], [80, 100]],
                        HysteresisC = 2.5,
                    },
                },
            },
            CpuLimit = new CpuLimitPart
            {
                Amd = new AmdCpuLimit { PptW = 88, TdcA = 75, EdcA = 120 },
                Intel = new JsonObject { ["pl1_w"] = 125 },
            },
            CurveOpt = new CurveOptPart { AllCore = -15, PerCore = new() { ["0"] = -20, ["1"] = -10 } },
            Gpu = new GpuPart { Adapters = new() { ["gpu-amd-0"] = new GpuAdapterConfig { PowerLimitPct = -10 } } },
            Aura = new AuraPart
            {
                Effect = "breathing",
                Color = "#ff0033",
                Brightness = 0.75,
                Lamp = new LampPart { On = true, Brightness = 0.5, Temperature = 4000 },
            },
            Mouse = new MousePart { Profile = 2 },
            Keyboard = new KeyboardPart { Profile = 3 },
            Headset = new HeadsetPart { EqPreset = "FPS" },
        },
    };

    [Fact]
    public void A_profile_serializes_as_the_shared_contract_example()
    {
        ContractExample.Check("control-profile.json", JsonSerializer.Serialize(FullProfile(), ControlJson.Options));
    }

    private static readonly JsonSerializerOptions Strict =
        new(ControlJson.Options) { UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow };

    /// The other way: what the app sends (it writes the same example back,
    /// `control.rs`) is read with nothing left over — a field only the app
    /// has would otherwise be dropped on save without a word.
    [Fact]
    public void The_shared_profile_example_reads_back_with_no_field_left_over()
    {
        var json = File.ReadAllText(ContractExample.PathOf("control-profile.json")).Trim();

        var profile = JsonSerializer.Deserialize<Profile>(json, Strict)!;

        Assert.Equal(json, JsonSerializer.Serialize(profile, ControlJson.Options));
    }

    [Fact]
    public void An_apply_result_serializes_as_the_shared_contract_example()
    {
        var result = ApplyResult.Success("my-rig", "Profile My rig applied; the keyboard gets it when it wakes.");
        ContractExample.Check("control-apply-result.json", JsonSerializer.Serialize(result, ControlJson.Options));
    }

    [Fact]
    public void The_headset_list_serializes_as_the_shared_contract_example()
    {
        var lighting = new LightingProvider([new FakeHeadset()], "", () => null, dryRun: false);
        ContractExample.Check("control-headset-settings.json",
            JsonSerializer.Serialize(Control.ControlPipeWorker.HeadsetsReply(lighting), ControlJson.Options));
    }

    [Fact]
    public void The_keyboard_list_serializes_as_the_shared_contract_example()
    {
        var lighting = new LightingProvider([new FakeKeyboard()], "", () => null, dryRun: false);
        ContractExample.Check("control-keyboard-settings.json",
            JsonSerializer.Serialize(Control.ControlPipeWorker.KeyboardsReply(lighting), ControlJson.Options));
    }

    /// A headset with settings and an equalizer, so every field is sent.
    private sealed class FakeHeadset : ILightingDevice, IHeadsetSettingsDevice, IHeadsetEqDevice
    {
        public string Id => "asus-headset-1afa-1";
        public string Name => "ROG Delta II";
        public string Kind => "headset";
        public string Firmware => "";
        public string? Blocked => null;
        public IReadOnlyList<AuraZone> Zones { get; } = [new("lights", "Lights", false, 0, 1)];
        public void Apply(AuraEffect effect, byte red, byte green, byte blue) { }
        public bool Release() => false;
        public JsonObject Diagnostics() => [];

        public HeadsetLimits? Limits => new(20, 3);
        public bool SettingsVerified => true;
        public HeadsetSettings? ReadSettings() => new(true, 8, true, 2);
        public HeadsetSettings WriteSettings(HeadsetSettings wanted) => wanted;

        public HeadsetEqLimits? EqLimits => new(100, 2, [new("Flat", [50, 50, 50, 50, 50, 50, 50, 50, 50, 50]), new("FPS", [40, 45, 50, 55, 60, 65, 60, 55, 50, 45])]);
        public bool EqVerified => true;
        public HeadsetEq? ReadEq() => new(true, [50, 50, 60, 50, 50, 50, 50, 50, 50, 40]);
        public HeadsetEq WriteEq(HeadsetEq wanted) => wanted;
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
        public KeyboardSettings? ReadSettings() => new(3, 1, 20);
        public KeyboardSettings WriteSettings(KeyboardSettings wanted) => wanted;
    }
}
