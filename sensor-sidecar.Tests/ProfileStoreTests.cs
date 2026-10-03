using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// CRUD round-trip against a fresh temp directory per test — never the real
/// <c>%ProgramData%</c> path (see <see cref="ProfileStore(string)"/>'s test
/// seam).
/// </summary>
public sealed class ProfileStoreTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), "rigstats-profilestore-tests-" + Guid.NewGuid());
    private readonly string _path;
    private readonly ProfileStore _store;

    public ProfileStoreTests()
    {
        Directory.CreateDirectory(_dir);
        _path = Path.Combine(_dir, "profiles.json");
        _store = new ProfileStore(_path);
    }

    public void Dispose()
    {
        try { Directory.Delete(_dir, recursive: true); } catch { }
    }

    [Fact]
    public async Task First_load_seeds_the_four_builtin_profiles_and_persists_them()
    {
        var profiles = await _store.ListAsync(CancellationToken.None);

        Assert.Equal(4, profiles.Count);
        Assert.Contains(profiles, p => p.Id == "silent" && p.Builtin);
        Assert.Contains(profiles, p => p.Id == "balanced" && p.Builtin);
        Assert.Contains(profiles, p => p.Id == "gaming" && p.Builtin);
        Assert.Contains(profiles, p => p.Id == "eco" && p.Builtin);
        Assert.True(File.Exists(_path));
    }

    [Fact]
    public async Task SaveProfile_then_get_round_trips_a_custom_profile()
    {
        var custom = new Profile
        {
            Id = "my-custom",
            Name = "My Custom",
            Builtin = false,
            Part = new ProfilePart { PowerPlan = "high_performance" },
        };

        await _store.SaveProfileAsync(custom, CancellationToken.None);
        var loaded = await _store.GetAsync("my-custom", CancellationToken.None);

        Assert.NotNull(loaded);
        Assert.Equal("My Custom", loaded!.Name);
        Assert.Equal("high_performance", loaded.Part.PowerPlan);
    }

    [Fact]
    public async Task SaveProfile_overwrites_an_existing_profile_with_the_same_id()
    {
        var v1 = new Profile { Id = "x", Name = "V1", Part = new ProfilePart { PowerPlan = "balanced" } };
        var v2 = new Profile { Id = "x", Name = "V2", Part = new ProfilePart { PowerPlan = "power_saver" } };

        await _store.SaveProfileAsync(v1, CancellationToken.None);
        await _store.SaveProfileAsync(v2, CancellationToken.None);

        var all = await _store.ListAsync(CancellationToken.None);
        Assert.Single(all, p => p.Id == "x");
        Assert.Equal("V2", (await _store.GetAsync("x", CancellationToken.None))!.Name);
    }

    [Fact]
    public async Task DeleteProfile_removes_a_custom_profile()
    {
        await _store.SaveProfileAsync(
            new Profile { Id = "temp", Name = "Temp", Part = new ProfilePart { PowerPlan = "balanced" } },
            CancellationToken.None);

        var deleted = await _store.DeleteProfileAsync("temp", CancellationToken.None);

        Assert.True(deleted);
        Assert.Null(await _store.GetAsync("temp", CancellationToken.None));
    }

    [Fact]
    public async Task DeleteProfile_refuses_to_delete_a_builtin_profile()
    {
        await Assert.ThrowsAsync<InvalidOperationException>(
            () => _store.DeleteProfileAsync("gaming", CancellationToken.None));

        Assert.NotNull(await _store.GetAsync("gaming", CancellationToken.None));
    }

    [Fact]
    public async Task DeleteProfile_returns_false_for_an_unknown_id()
    {
        var deleted = await _store.DeleteProfileAsync("does-not-exist", CancellationToken.None);
        Assert.False(deleted);
    }

    [Fact]
    public async Task SetActive_persists_across_a_fresh_store_instance_reading_the_same_file()
    {
        await _store.SetActiveAsync("gaming", CancellationToken.None);

        var reopened = new ProfileStore(_path);
        Assert.Equal("gaming", await reopened.GetActiveIdAsync(CancellationToken.None));
    }

    [Fact]
    public async Task DeleteProfile_falls_back_active_to_another_profile_when_the_active_one_is_deleted()
    {
        await _store.SaveProfileAsync(
            new Profile { Id = "temp", Name = "Temp", Part = new ProfilePart { PowerPlan = "balanced" } },
            CancellationToken.None);
        await _store.SetActiveAsync("temp", CancellationToken.None);

        await _store.DeleteProfileAsync("temp", CancellationToken.None);

        var active = await _store.GetActiveIdAsync(CancellationToken.None);
        Assert.Equal("balanced", active); // the neutral fallback, not the first (Silent)
    }

    [Fact]
    public async Task Builtins_put_every_fan_header_on_bios_control()
    {
        var profiles = await _store.ListAsync(CancellationToken.None);

        Assert.All(profiles, p => Assert.Empty(p.Part.Fan!.Headers!));
    }

    [Fact]
    public async Task Builtins_saved_before_fan_support_gain_the_empty_fan_part()
    {
        await File.WriteAllTextAsync(_path, """
            {"active":"gaming","profiles":[
              {"id":"gaming","name":"Gaming","builtin":true,"part":{"power_plan":"high_performance"}},
              {"id":"mine","name":"Mine","part":{"power_plan":"balanced"}}]}
            """);

        var profiles = await new ProfileStore(_path).ListAsync(CancellationToken.None);

        var gaming = profiles.Single(p => p.Id == "gaming");
        Assert.Equal("high_performance", gaming.Part.PowerPlan);
        Assert.Empty(gaming.Part.Fan!.Headers!);
        Assert.Null(profiles.Single(p => p.Id == "mine").Part.Fan); // user profiles are left as saved
    }

    [Fact]
    public async Task Builtins_saved_before_cpu_and_gpu_limits_gain_empty_parts_and_keep_their_fans()
    {
        await File.WriteAllTextAsync(_path, """
            {"active":"silent","profiles":[
              {"id":"silent","name":"Silent","builtin":true,"part":{"fan":{"headers":{"lpc/x/control/1":{"source":"cpu_package","curve":[[40,30],[80,100]]}}}}}]}
            """);

        var silent = (await new ProfileStore(_path).ListAsync(CancellationToken.None)).Single();

        Assert.NotNull(silent.Part.CpuLimit);
        Assert.Null(silent.Part.CpuLimit.Amd); // = BIOS values
        Assert.NotNull(silent.Part.Gpu);
        Assert.Null(silent.Part.Gpu.Adapters); // = driver / original values
        Assert.NotNull(silent.Part.CurveOpt);
        Assert.Null(silent.Part.CurveOpt.AllCore); // = BIOS values
        Assert.Single(silent.Part.Fan!.Headers!);
    }

    [Fact]
    public async Task ResetBuiltin_restores_the_defaults_and_refuses_custom_profiles()
    {
        var gaming = await _store.GetAsync("gaming", CancellationToken.None);
        await _store.SaveProfileAsync(new Profile
        {
            Id = "gaming",
            Name = "My Gaming",
            Icon = gaming!.Icon,
            Builtin = true,
            Part = new ProfilePart { PowerPlan = "power_saver", CpuLimit = new CpuLimitPart { Amd = new AmdCpuLimit { PptW = 88 } } },
        }, CancellationToken.None);
        await _store.SaveProfileAsync(new Profile { Id = "mine", Name = "Mine", Part = new ProfilePart() }, CancellationToken.None);

        var reset = await _store.ResetBuiltinAsync("gaming", CancellationToken.None);

        Assert.Equal("Gaming", reset!.Name);
        var stored = await new ProfileStore(_path).GetAsync("gaming", CancellationToken.None);
        Assert.Equal("high_performance", stored!.Part.PowerPlan);
        Assert.Null(stored.Part.CpuLimit!.Amd);
        Assert.Null(await _store.ResetBuiltinAsync("mine", CancellationToken.None));
    }

    [Fact]
    public async Task Saving_an_existing_profile_keeps_its_place_and_new_ones_go_last()
    {
        var before = (await _store.ListAsync(CancellationToken.None)).Select(p => p.Id).ToList();
        var silent = await _store.GetAsync("silent", CancellationToken.None);

        await _store.SaveProfileAsync(new Profile
        {
            Id = "silent",
            Name = "Quiet",
            Icon = silent!.Icon,
            Builtin = true,
            Part = silent.Part,
        }, CancellationToken.None);
        await _store.SaveProfileAsync(new Profile { Id = "mine", Name = "Mine", Part = new ProfilePart() }, CancellationToken.None);

        var after = (await new ProfileStore(_path).ListAsync(CancellationToken.None)).Select(p => p.Id).ToList();
        Assert.Equal(before.Append("mine"), after);
    }
}
