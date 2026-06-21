using LibreHardwareMonitor.Hardware;
using NSubstitute;
using SensorSidecar;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Exercises the real <see cref="SensorReader.Extract"/> filtering rules against
/// mocked LibreHardwareMonitor hardware trees (one rule per test).
/// </summary>
public class SensorReaderTests
{
    // ---- builders -----------------------------------------------------------

    private static Identifier Id(string path) =>
        new(path.Split('/', StringSplitOptions.RemoveEmptyEntries));

    private static ISensor Sensor(SensorType type, string name, float? value, string id = "/x/0")
    {
        var s = Substitute.For<ISensor>();
        s.SensorType.Returns(type);
        s.Name.Returns(name);
        s.Value.Returns(value);
        s.Identifier.Returns(Id(id));
        return s;
    }

    private static IHardware Hw(
        HardwareType type, string name, string id,
        ISensor[] sensors, IHardware[]? sub = null)
    {
        var hw = Substitute.For<IHardware>();
        hw.HardwareType.Returns(type);
        hw.Name.Returns(name);
        hw.Identifier.Returns(Id(id));
        hw.Sensors.Returns(sensors);
        hw.SubHardware.Returns(sub ?? []);
        return hw;
    }

    private static IComputer Computer(params IHardware[] hw)
    {
        var c = Substitute.For<IComputer>();
        c.Hardware.Returns(hw);
        return c;
    }

    // ---- CPU ----------------------------------------------------------------

    [Fact]
    public void ExtractCpu_picks_temp_and_power_by_name_and_skips_null()
    {
        var cpu = Hw(HardwareType.Cpu, "AMD Ryzen", "/amdcpu/0",
        [
            Sensor(SensorType.Temperature, "Core (Tctl/Tdie)", 55.0f),
            Sensor(SensorType.Temperature, "Core #1", 99.0f), // not a mapped name
            Sensor(SensorType.Power, "CPU Package", 42.0f),
            Sensor(SensorType.Power, "Package", null), // null skipped
        ]);

        var p = SensorReader.Extract(Computer(cpu));

        Assert.Equal(55.0f, p.CpuTemp);
        Assert.Equal(42.0f, p.CpuPower);
    }

    [Fact]
    public void ExtractCpu_accepts_cpu_package_temperature_name()
    {
        var cpu = Hw(HardwareType.Cpu, "Intel", "/intelcpu/0",
        [
            Sensor(SensorType.Temperature, "CPU Package", 61.0f),
            Sensor(SensorType.Power, "Package", 30.0f),
        ]);

        var p = SensorReader.Extract(Computer(cpu));

        Assert.Equal(61.0f, p.CpuTemp);
        Assert.Equal(30.0f, p.CpuPower);
    }

    // ---- GPU ----------------------------------------------------------------

    [Fact]
    public void ExtractGpu_extracts_load_d3d_clocks_and_hotspot()
    {
        var gpu = Hw(HardwareType.GpuNvidia, "RTX 4080", "/gpu-nvidia/0",
        [
            Sensor(SensorType.Load, "GPU Core", 40.0f),
            Sensor(SensorType.Load, "D3D 3D", 25.0f),
            Sensor(SensorType.Load, "D3D Video Decode", 7.0f),
            Sensor(SensorType.Temperature, "GPU Core", 60.0f),
            Sensor(SensorType.Temperature, "GPU Hot Spot", 75.0f),
            Sensor(SensorType.Clock, "GPU Core", 2400.0f),
            Sensor(SensorType.Clock, "GPU Memory", 11000.0f),
            Sensor(SensorType.Power, "GPU Package", 180.0f),
        ]);

        var d = Assert.Single(SensorReader.Extract(Computer(gpu)).GpuDevices);

        Assert.Equal(40.0f, d.Load);
        Assert.Equal(25.0f, d.D3d3d);
        Assert.Equal(7.0f, d.D3dVdec);
        Assert.Equal(60.0f, d.Temp);
        Assert.Equal(75.0f, d.HotspotTemp);
        Assert.Equal(2400.0f, d.CoreClock);
        Assert.Equal(11000.0f, d.MemClock);
        Assert.Equal(180.0f, d.Power);
        Assert.Equal("gpu-nvidia", d.SensorFamily);
    }

    [Fact]
    public void ExtractGpu_falls_back_to_vr_soc_temp_only_when_core_temp_missing()
    {
        var gpu = Hw(HardwareType.GpuAmd, "RX 7900", "/gpu-amd/0",
        [
            Sensor(SensorType.Temperature, "GPU VR SoC", 58.0f),
        ]);

        var d = Assert.Single(SensorReader.Extract(Computer(gpu)).GpuDevices);
        Assert.Equal(58.0f, d.Temp);
    }

    [Fact]
    public void ExtractGpu_prefers_core_temp_over_vr_soc()
    {
        var gpu = Hw(HardwareType.GpuAmd, "RX 7900", "/gpu-amd/0",
        [
            Sensor(SensorType.Temperature, "GPU VR SoC", 58.0f),
            Sensor(SensorType.Temperature, "GPU Core", 64.0f),
        ]);

        var d = Assert.Single(SensorReader.Extract(Computer(gpu)).GpuDevices);
        Assert.Equal(64.0f, d.Temp);
    }

    [Fact]
    public void ExtractGpu_amd_igpu_power_sums_core_and_soc_when_no_package()
    {
        var gpu = Hw(HardwareType.GpuAmd, "Radeon 780M", "/gpu-amd/0",
        [
            Sensor(SensorType.Power, "GPU Core", 8.0f),
            Sensor(SensorType.Power, "GPU SoC", 4.0f),
        ]);

        var d = Assert.Single(SensorReader.Extract(Computer(gpu)).GpuDevices);
        Assert.Equal(12.0f, d.Power);
    }

    [Fact]
    public void ExtractGpu_amd_video_decode_matches_naming_variants()
    {
        // NVIDIA: "D3D Video Decode"; AMD iGPU: "D3D Video Decode 1";
        // AMD discrete: unified "D3D Video Codec Engine". All must populate d3d_vdec.
        var igpu = Hw(HardwareType.GpuAmd, "Radeon iGPU", "/gpu-amd/0",
        [
            Sensor(SensorType.Load, "D3D Video Decode 1", 12.0f),
            Sensor(SensorType.Load, "D3D Video Codec Engine", 3.0f), // max wins
        ]);
        var dgpu = Hw(HardwareType.GpuAmd, "Radeon dGPU", "/gpu-amd/5",
        [
            Sensor(SensorType.Load, "D3D Video Codec Engine", 0.0f),
        ]);

        var devices = SensorReader.Extract(Computer(igpu, dgpu)).GpuDevices;

        Assert.Equal(12.0f, devices[0].D3dVdec); // busiest decode/codec engine
        Assert.Equal(0.0f, devices[1].D3dVdec);  // present but idle → 0, not null
    }

    [Fact]
    public void ExtractGpu_reads_vram_from_smalldata_mb_and_data_gb()
    {
        var mbGpu = Hw(HardwareType.GpuNvidia, "A", "/gpu-nvidia/0",
        [
            Sensor(SensorType.SmallData, "GPU Memory Used", 4096.0f),
            Sensor(SensorType.SmallData, "GPU Memory Total", 16384.0f),
        ]);
        var gbGpu = Hw(HardwareType.GpuIntel, "B", "/gpu-intel/0",
        [
            Sensor(SensorType.Data, "GPU Memory Used", 2.0f),
            Sensor(SensorType.Data, "GPU Memory Total", 8.0f),
        ]);

        var devices = SensorReader.Extract(Computer(mbGpu, gbGpu)).GpuDevices;

        Assert.Equal(4096.0f, devices[0].VramUsedMb);
        Assert.Equal(16384.0f, devices[0].VramTotalMb);
        Assert.Equal(2048.0f, devices[1].VramUsedMb);
        Assert.Equal(8192.0f, devices[1].VramTotalMb);
    }

    // ---- Disk ---------------------------------------------------------------

    [Fact]
    public void ExtractDisk_includes_only_storage_identifiers_and_highest_temp()
    {
        var nvme = Hw(HardwareType.Storage, "Samsung 990", "/nvme/0",
        [
            Sensor(SensorType.Temperature, "Temperature", 40.0f),
            Sensor(SensorType.Temperature, "Temperature 2", 45.0f), // highest wins
            Sensor(SensorType.Temperature, "Temperature 3 (Warning)", 80.0f), // excluded
            Sensor(SensorType.Temperature, "Critical Composite", 90.0f), // excluded
        ]);
        var weird = Hw(HardwareType.Storage, "Ghost", "/unknownbus/0",
        [
            Sensor(SensorType.Temperature, "Temperature", 50.0f),
        ]);

        var p = SensorReader.Extract(Computer(nvme, weird));

        Assert.Equal(45.0f, p.DiskTemps["Samsung 990"]);
        Assert.False(p.DiskTemps.ContainsKey("Ghost"));
    }

    // ---- RAM ----------------------------------------------------------------

    [Fact]
    public void ExtractRam_takes_max_across_dimm_temperature0_only()
    {
        var ram = Hw(HardwareType.Memory, "RAM", "/ram/0",
        [
            Sensor(SensorType.Temperature, "DIMM #1", 38.0f, "/memory/0/temperature/0"),
            Sensor(SensorType.Temperature, "DIMM #2", 41.0f, "/memory/1/temperature/0"),
            // a non-temperature/0 index must be ignored even if hotter
            Sensor(SensorType.Temperature, "DIMM #3", 99.0f, "/memory/2/temperature/1"),
        ]);

        var p = SensorReader.Extract(Computer(ram));
        Assert.Equal(41.0f, p.RamTemp);
    }

    [Fact]
    public void ExtractRam_none_when_no_dimm_temperature_sensor()
    {
        var ram = Hw(HardwareType.Memory, "RAM", "/ram/0",
        [
            Sensor(SensorType.Data, "Memory Used", 8192.0f, "/memory/0/data/0"),
        ]);

        var p = SensorReader.Extract(Computer(ram));
        Assert.Null(p.RamTemp);
    }

    // ---- Motherboard --------------------------------------------------------

    [Fact]
    public void ExtractMotherboard_filters_and_sorts_lpc_sensors()
    {
        var lpc = Hw(HardwareType.SuperIO, "Nuvoton NCT6799D", "/lpc/nct6799d/0",
        [
            Sensor(SensorType.Fan, "CPU Fan", 900.0f),
            Sensor(SensorType.Fan, "Chassis Fan", 1500.0f),
            Sensor(SensorType.Fan, "Unused Fan", 0.0f), // 0 rpm excluded
            Sensor(SensorType.Temperature, "System", 35.0f),
            Sensor(SensorType.Temperature, "Cold Sensor", 1.0f), // < 5 excluded
            Sensor(SensorType.Voltage, "Vcore", 1.25f),
            Sensor(SensorType.Voltage, "Voltage #5", 0.5f), // generic slot excluded
            Sensor(SensorType.Voltage, "AVCC", 0.05f), // <= 0.1 excluded
        ]);
        var mb = Hw(HardwareType.Motherboard, "X670", "/mainboard", [], [lpc]);

        // GPU hardware must not bleed into the motherboard lists.
        var gpu = Hw(HardwareType.GpuNvidia, "GPU", "/gpu-nvidia/0",
            [Sensor(SensorType.Fan, "GPU Fan", 2000.0f)]);

        var p = SensorReader.Extract(Computer(mb, gpu));

        Assert.Equal("Nuvoton NCT6799D", p.MbChip);
        Assert.Equal(2, p.MbFans.Count);
        Assert.Equal("Chassis Fan", p.MbFans[0].Label); // sorted descending
        Assert.Equal("CPU Fan", p.MbFans[1].Label);
        Assert.Single(p.MbTemps);
        Assert.Equal("System", p.MbTemps[0].Label);
        Assert.Single(p.MbVoltages);
        Assert.Equal("Vcore", p.MbVoltages[0].Label);
    }

    [Fact]
    public void ExtractMotherboard_ignores_non_lpc_subhardware()
    {
        var nonLpc = Hw(HardwareType.SuperIO, "Embedded", "/embedded/0",
            [Sensor(SensorType.Fan, "Fan", 1000.0f)]);
        var mb = Hw(HardwareType.Motherboard, "Board", "/mainboard", [], [nonLpc]);

        var p = SensorReader.Extract(Computer(mb));

        Assert.Null(p.MbChip);
        Assert.Empty(p.MbFans);
    }
}
