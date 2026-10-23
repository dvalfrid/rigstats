using System.Text.Json;
using System.Text.Json.Nodes;
using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Real NVML power limits (#572): `gpu-power.json` from users' diagnostics
/// ZIPs, in <c>fixtures/gpu-power/*.json</c>. Each GPU's raw limits go
/// through <see cref="NvmlGpuPower.Describe"/> and
/// <see cref="NvmlGpuPower.IsLaptopGpu"/> and must give the `range_pct` and
/// `offered` the service wrote — what the Graphics card page offers on that
/// card. See fixtures/README.md, "NVML power limits".
/// </summary>
public class GpuPowerFixtureTests
{
    private static string Root => Path.Combine(AppContext.BaseDirectory, "fixtures", "gpu-power");

    public static IEnumerable<object[]> Files() =>
        Directory.Exists(Root)
            ? Directory.EnumerateFiles(Root, "*.json").Select(f => new object[] { Path.GetFileName(f) })
            : [];

    [Theory]
    [MemberData(nameof(Files))]
    public void Offers_each_card_as_recorded(string file)
    {
        var doc = JsonNode.Parse(File.ReadAllText(Path.Combine(Root, file)))!;
        var gpus = doc["nvml"]?["gpus"]?.AsArray() ?? [];
        Assert.NotEmpty(gpus);
        foreach (var gpu in gpus.Select(g => g!))
        {
            var name = gpu["name"]!.GetValue<string>();
            // A reading NVML refused is its error's name instead of a number.
            uint? Mw(string key) => gpu[key] is JsonValue v && v.GetValueKind() == JsonValueKind.Number ? v.GetValue<uint>() : null;
            var adapter = Mw("min_mw") is { } min && Mw("max_mw") is { } max && Mw("default_mw") is { } def && Mw("limit_mw") is not null
                ? NvmlGpuPower.Describe("fixture", name, min, max, def)
                : null;
            Assert.Equal(gpu["laptop"]!.GetValue<bool>(), NvmlGpuPower.IsLaptopGpu(name));
            Assert.Equal(gpu["range_pct"]?.GetValue<string>(), adapter is null ? null : $"{adapter.Min}..{adapter.Max}");
            var offered = adapter is not null && !NvmlGpuPower.IsLaptopGpu(name) && !gpu["refused_write"]!.GetValue<bool>();
            Assert.Equal(gpu["offered"]!.GetValue<bool>(), offered);
        }
    }
}
