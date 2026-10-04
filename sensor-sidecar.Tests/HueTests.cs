using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;
using System.Text.Json.Nodes;
using SensorSidecar.Control;
using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>Philips Hue via the Hue Bridge (#215): colour maths, commands, discovery.</summary>
public class HueTests
{
    [Fact]
    public void Srgb_primaries_land_on_their_xy()
    {
        var (x, y, brightness) = HueColor.FromRgb(255, 0, 0);
        Assert.Equal(0.64, x, 0.005);
        Assert.Equal(0.33, y, 0.005);
        Assert.Equal(100, brightness);

        (x, y, _) = HueColor.FromRgb(0, 255, 0);
        Assert.Equal(0.30, x, 0.005);
        Assert.Equal(0.60, y, 0.005);
    }

    [Fact]
    public void White_is_d65_and_brightness_follows_the_brightest_channel()
    {
        var (x, y, brightness) = HueColor.FromRgb(255, 255, 255);
        Assert.Equal(HueColor.White.X, x, 0.002);
        Assert.Equal(HueColor.White.Y, y, 0.002);
        Assert.Equal(100, brightness);

        // The profile's brightness arrives as a scaled colour: same xy, dimmer.
        var dim = HueColor.FromRgb(128, 0, 0);
        Assert.Equal(HueColor.FromRgb(255, 0, 0).X, dim.X, 0.001);
        Assert.Equal(50.2, dim.Brightness, 0.1);
        Assert.Equal(0, HueColor.FromRgb(0, 0, 0).Brightness);
    }

    [Fact]
    public void A_colour_outside_gamut_c_moves_to_its_edge()
    {
        // sRGB blue (0.15, 0.06) is just outside gamut C's green–blue edge.
        var (x, y, _) = HueColor.FromRgb(0, 0, 255);
        Assert.Equal(0.1535, x, 0.001);
        Assert.Equal(0.06, y, 0.002);

        // Inside stays put.
        Assert.Equal((0.4, 0.4), HueColor.Clamp((0.4, 0.4)));
    }

    [Fact]
    public void Off_and_black_switch_the_lights_off()
    {
        Assert.False(HueRoomDevice.Command(AuraEffect.Off, 255, 0, 0, TimeSpan.Zero)["on"]!["on"]!.GetValue<bool>());
        Assert.False(HueRoomDevice.Command(AuraEffect.Static, 0, 0, 0, TimeSpan.Zero)["on"]!["on"]!.GetValue<bool>());
    }

    [Fact]
    public void Static_is_one_short_fade_to_the_colour()
    {
        var command = HueRoomDevice.Command(AuraEffect.Static, 255, 0, 0, TimeSpan.Zero);
        Assert.True(command["on"]!["on"]!.GetValue<bool>());
        Assert.Equal(100, command["dimming"]!["brightness"]!.GetValue<double>());
        Assert.Equal(0.64, command["color"]!["xy"]!["x"]!.GetValue<double>(), 0.005);
        Assert.Equal(400, command["dynamics"]!["duration"]!.GetValue<int>());
    }

    [Fact]
    public void Breathing_fades_between_full_and_the_floor_one_step_at_a_time()
    {
        var step = HueRoomDevice.AnimationStep;
        // Starts dark, so the first fade goes up to full …
        var up = HueRoomDevice.Command(AuraEffect.Breathing, 255, 0, 0, TimeSpan.Zero);
        Assert.Equal(100, up["dimming"]!["brightness"]!.GetValue<double>(), 0.5);
        Assert.Equal((int)step.TotalMilliseconds, up["dynamics"]!["duration"]!.GetValue<int>());
        // … the next one down to the floor, never off.
        var down = HueRoomDevice.Command(AuraEffect.Breathing, 255, 0, 0, step);
        Assert.Equal(1, down["dimming"]!["brightness"]!.GetValue<double>());
        Assert.True(down["on"]!["on"]!.GetValue<bool>());
        // The colour stays the profile's.
        Assert.Equal(up["color"]!.ToJsonString(), down["color"]!.ToJsonString());
    }

    [Fact]
    public void Spectrum_cycle_moves_the_colour_and_keeps_the_brightness()
    {
        var a = HueRoomDevice.Command(AuraEffect.SpectrumCycle, 128, 128, 128, TimeSpan.Zero);
        var b = HueRoomDevice.Command(AuraEffect.SpectrumCycle, 128, 128, 128, HueRoomDevice.AnimationStep);
        Assert.NotEqual(a["color"]!.ToJsonString(), b["color"]!.ToJsonString());
        Assert.Equal(50.2, a["dimming"]!["brightness"]!.GetValue<double>(), 0.1);
        Assert.Equal(50.2, b["dimming"]!["brightness"]!.GetValue<double>(), 0.1);
    }

    [Fact]
    public void Room_device_sends_on_its_own_thread_and_the_newest_wins()
    {
        var sent = new List<JsonObject>();
        var gate = new ManualResetEventSlim();
        var group = new HueGroup("room-1", "Office", "room", "grouped-1");
        var bridge = new HueBridgeInfo("001788fffe000001", "Hue Bridge", "BSB002", "1967054020", "192.0.2.1");
        using var device = new HueRoomDevice(group, bridge, (id, body) =>
        {
            Assert.Equal("grouped-1", id);
            lock (sent)
                sent.Add(body);
            gate.Set();
        });
        Assert.Equal("hue-room-1", device.Id);
        Assert.Equal("Hue: Office", device.Name);

        device.Apply(AuraEffect.Static, 0, 255, 0);
        Assert.True(gate.Wait(TimeSpan.FromSeconds(5)));
        lock (sent)
            Assert.True(sent[^1]["on"]!["on"]!.GetValue<bool>());
        Assert.DoesNotContain("key", device.Diagnostics().ToJsonString());
    }

    [Fact]
    public void A_failing_room_reports_its_problem_until_a_write_works()
    {
        var fail = true;
        var sends = new SemaphoreSlim(0);
        using var device = new HueRoomDevice(
            new HueGroup("room-1", "Office", "room", "grouped-1"),
            new HueBridgeInfo("id", "Hue Bridge", "BSB002", "1", "192.0.2.1"),
            (_, _) =>
            {
                try
                {
                    if (Volatile.Read(ref fail))
                        throw new InvalidOperationException("The Hue Bridge at 192.0.2.1 can't be reached.");
                }
                finally
                {
                    sends.Release();
                }
            });
        Assert.Null(device.Problem);

        device.Apply(AuraEffect.Static, 255, 0, 0);
        Assert.True(sends.Wait(TimeSpan.FromSeconds(5)));
        Assert.True(SpinWait.SpinUntil(() => device.Problem is not null, TimeSpan.FromSeconds(5)));
        Assert.Contains("can't be reached", device.Problem);

        Volatile.Write(ref fail, false);
        device.Apply(AuraEffect.Static, 0, 255, 0);
        Assert.True(SpinWait.SpinUntil(() => device.Problem is null, TimeSpan.FromSeconds(5)));
    }

    [Fact]
    public void Mdns_query_asks_for_hue_bridges_and_only_their_answers_count()
    {
        var query = HueBridge.MdnsQuery;
        Assert.Equal(1, query[5]); // one question
        Assert.Equal(12, query[^3]); // PTR

        // A response repeating the question.
        var answer = query.ToArray();
        answer[2] = 0x84;
        Assert.True(HueBridge.IsHueAnswer(answer));
        // Our own question echoed back, or another service's answer, isn't.
        Assert.False(HueBridge.IsHueAnswer(query));
        var other = answer.ToArray();
        other[13] = (byte)'x';
        Assert.False(HueBridge.IsHueAnswer(other));
    }

    [Fact]
    public void A_certificate_not_signed_by_hue_is_refused()
    {
        using var key = ECDsa.Create(ECCurve.NamedCurves.nistP256);
        var request = new CertificateRequest("CN=001788fffe000001", key, HashAlgorithmName.SHA256);
        using var selfSigned = request.CreateSelfSigned(DateTimeOffset.UtcNow.AddDays(-1), DateTimeOffset.UtcNow.AddYears(1));
        Assert.False(HueBridge.ChainsToHueRoot(selfSigned, null));
        // Said in words, not as a bare TLS failure.
        Assert.Contains("isn't signed by Philips Hue", HueBridge.CertificateRefusal(selfSigned, null, null, out _));
        Assert.Equal("it sent no certificate", HueBridge.CertificateRefusal(null, null, null, out _));
    }

    [Fact]
    public void Unpaired_link_has_no_devices_and_says_so()
    {
        var link = new HueLink(Path.Combine(Path.GetTempPath(), $"rigstats-hue-{Guid.NewGuid():N}.json"));
        Assert.Empty(link.Devices());
        Assert.False(link.Details()["paired"]!.GetValue<bool>());
    }

    [Fact]
    public void Rediscover_finds_devices_though_no_hid_collection_changed()
    {
        var hueDevices = new List<ILightingDevice>();
        var link = new HueLink(Path.Combine(Path.GetTempPath(), $"rigstats-hue-{Guid.NewGuid():N}.json"));
        var provider = new LightingProvider(() => [], _ => new LightingScan(hueDevices.ToList(), "none"), () => null, dryRun: true, hue: link);
        Assert.Empty(provider.Devices);
        // Unsupported without devices, but the Hue details go along for pairing.
        Assert.NotNull(provider.Probe().Details?["hue"]);

        hueDevices.Add(new HueRoomDevice(
            new HueGroup("room-1", "Office", "room", "grouped-1"),
            new HueBridgeInfo("id", "Hue Bridge", "BSB002", "1", "192.0.2.1"),
            (_, _) => { }));
        provider.Rescan(force: true);
        Assert.Empty(provider.Devices); // same HID collections: nothing new looked for
        provider.Rediscover();
        Assert.Single(provider.Devices);
    }
}
