using System.Text.Json;
using System.Text.Json.Nodes;
using SensorSidecar;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Cross-language contract tests. These lock the exact snake_case JSON property
/// names the Rust <c>SidecarPayload</c> deserializer in
/// <c>rigstats-backend/src/lhm.rs</c> depends on. A C#-side rename of any field
/// now fails here instead of silently breaking deserialization at runtime.
/// </summary>
public class SerializationTests
{
    // Mirrors SensorWorker._jsonOptions exactly.
    private static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
    };

    private static SensorPayload FullyPopulated() => new(
        CpuTemp: 55.5f,
        CpuPower: 42.0f,
        GpuDevices:
        [
            new GpuDevice(
                Name: "NVIDIA GeForce RTX 4080",
                SensorFamily: "gpu-nvidia",
                Load: 33.0f,
                Temp: 60.0f,
                HotspotTemp: 72.0f,
                CoreClock: 2400.0f,
                MemClock: 11000.0f,
                Power: 180.0f,
                Fan: 1200.0f,
                VramUsedMb: 4096.0f,
                VramTotalMb: 16384.0f,
                D3d3d: 25.0f,
                D3dVdec: 5.0f),
        ],
        DiskTemps: new Dictionary<string, float> { ["Samsung SSD 990"] = 41.0f },
        RamTemp: 38.0f,
        MbFans: [new MbFan("CPU Fan", 900.0f)],
        MbTemps: [new MbTemp("System", 35.0f)],
        MbVoltages: [new MbVoltage("Vcore", 1.25f)],
        MbChip: "Nuvoton NCT6799D");

    private static JsonObject SerializeToObject(SensorPayload payload)
    {
        var json = JsonSerializer.Serialize(payload, Options);
        return JsonNode.Parse(json)!.AsObject();
    }

    [Theory]
    [InlineData("cpu_temp")]
    [InlineData("cpu_power")]
    [InlineData("gpu_devices")]
    [InlineData("disk_temps")]
    [InlineData("ram_temp")]
    [InlineData("mb_fans")]
    [InlineData("mb_temps")]
    [InlineData("mb_voltages")]
    [InlineData("mb_chip")]
    public void Payload_has_top_level_property(string name)
    {
        var obj = SerializeToObject(FullyPopulated());
        Assert.True(obj.ContainsKey(name), $"missing top-level property '{name}'");
    }

    [Theory]
    [InlineData("name")]
    [InlineData("load")]
    [InlineData("temp")]
    [InlineData("hotspot_temp")]
    [InlineData("core_clock")]
    [InlineData("mem_clock")]
    [InlineData("power")]
    [InlineData("fan")]
    [InlineData("vram_used_mb")]
    [InlineData("vram_total_mb")]
    [InlineData("d3d_3d")]
    [InlineData("d3d_vdec")]
    public void GpuDevice_has_property(string name)
    {
        var obj = SerializeToObject(FullyPopulated());
        var gpu = obj["gpu_devices"]!.AsArray()[0]!.AsObject();
        Assert.True(gpu.ContainsKey(name), $"missing gpu_devices[0] property '{name}'");
    }

    [Fact]
    public void MbFan_uses_label_and_rpm()
    {
        var obj = SerializeToObject(FullyPopulated());
        var fan = obj["mb_fans"]!.AsArray()[0]!.AsObject();
        Assert.True(fan.ContainsKey("label"));
        Assert.True(fan.ContainsKey("rpm"));
    }

    [Fact]
    public void MbTemp_uses_label_and_celsius()
    {
        var obj = SerializeToObject(FullyPopulated());
        var temp = obj["mb_temps"]!.AsArray()[0]!.AsObject();
        Assert.True(temp.ContainsKey("label"));
        Assert.True(temp.ContainsKey("celsius"));
    }

    [Fact]
    public void MbVoltage_uses_label_and_volts()
    {
        var obj = SerializeToObject(FullyPopulated());
        var volt = obj["mb_voltages"]!.AsArray()[0]!.AsObject();
        Assert.True(volt.ContainsKey("label"));
        Assert.True(volt.ContainsKey("volts"));
    }

    [Fact]
    public void D3d_fields_keep_explicit_snake_case_names()
    {
        // d3d_3d / d3d_vdec carry an explicit [JsonPropertyName]; verify the
        // naming policy does not mangle them (e.g. into "d3d3d").
        var obj = SerializeToObject(FullyPopulated());
        var gpu = obj["gpu_devices"]!.AsArray()[0]!.AsObject();
        Assert.Equal(25.0, gpu["d3d_3d"]!.GetValue<double>(), 3);
        Assert.Equal(5.0, gpu["d3d_vdec"]!.GetValue<double>(), 3);
    }

    [Fact]
    public void Null_optionals_serialize_as_json_null()
    {
        var payload = new SensorPayload(
            CpuTemp: null, CpuPower: null, GpuDevices: [],
            DiskTemps: new Dictionary<string, float>(), RamTemp: null,
            MbFans: [], MbTemps: [], MbVoltages: [], MbChip: null);
        var obj = SerializeToObject(payload);
        Assert.True(obj.ContainsKey("cpu_temp"));
        Assert.True(obj.ContainsKey("mb_chip"));
        // A JSON null is represented as a null JsonNode under the key.
        Assert.Null(obj["cpu_temp"]);
        Assert.Null(obj["mb_chip"]);
    }
}
