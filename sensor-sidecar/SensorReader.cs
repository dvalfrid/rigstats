using System.Text.Json.Serialization;
using LibreHardwareMonitor.Hardware;

namespace SensorSidecar;

public sealed record SensorPayload(
    float? CpuTemp,
    float? CpuPower,
    List<GpuDevice> GpuDevices,
    Dictionary<string, float> DiskTemps,
    float? RamTemp,
    List<MbFan> MbFans,
    List<MbTemp> MbTemps,
    List<MbVoltage> MbVoltages,
    string? MbChip);

public sealed record GpuDevice(
    string Name,
    string SensorFamily,
    float? Load,
    float? Temp,
    float? HotspotTemp,
    float? CoreClock,
    float? MemClock,
    float? Power,
    float? Fan,
    float? VramUsedMb,
    float? VramTotalMb,
    [property: JsonPropertyName("d3d_3d")] float? D3d3d,
    [property: JsonPropertyName("d3d_vdec")] float? D3dVdec);

public sealed record MbFan(string Label, float Rpm);
public sealed record MbTemp(string Label, float Celsius);
public sealed record MbVoltage(string Label, float Volts);

public static class SensorReader
{
    private static readonly string[] CpuTempNames =
        ["Core (Tctl/Tdie)", "CPU Package", "Core Average"];

    public static SensorPayload Extract(IComputer computer)
    {
        float? cpuTemp = null, cpuPower = null, ramTemp = null;
        var gpuDevices = new List<GpuDevice>();
        var diskTemps = new Dictionary<string, float>(StringComparer.OrdinalIgnoreCase);
        var mbFans = new List<MbFan>();
        var mbTemps = new List<MbTemp>();
        var mbVoltages = new List<MbVoltage>();
        string? mbChip = null;

        foreach (var hw in computer.Hardware)
        {
            switch (hw.HardwareType)
            {
                case HardwareType.Cpu:
                    ExtractCpu(hw, ref cpuTemp, ref cpuPower);
                    break;
                case HardwareType.GpuNvidia:
                case HardwareType.GpuAmd:
                case HardwareType.GpuIntel:
                    gpuDevices.Add(ExtractGpu(hw));
                    break;
                case HardwareType.Storage:
                    ExtractDisk(hw, diskTemps);
                    break;
                case HardwareType.Memory:
                    ExtractRam(hw, ref ramTemp);
                    break;
                case HardwareType.Motherboard:
                    ExtractMotherboard(hw, mbFans, mbTemps, mbVoltages, ref mbChip);
                    break;
            }
        }

        return new SensorPayload(cpuTemp, cpuPower, gpuDevices, diskTemps,
            ramTemp, mbFans, mbTemps, mbVoltages, mbChip);
    }

    private static void ExtractCpu(IHardware hw, ref float? temp, ref float? power)
    {
        foreach (var s in hw.Sensors)
        {
            if (s.Value is null) continue;
            // A literal 0 here means the SMU couldn't be read (seen on some AMD
            // Zen/Zen+ board/driver combos) rather than a real reading — treat it
            // as unavailable, same threshold as ExtractMotherboard's temp filter.
            if (s.SensorType == SensorType.Temperature && CpuTempNames.Contains(s.Name))
            {
                if (s.Value >= 5) temp = s.Value;
            }
            else if (s.SensorType == SensorType.Power &&
                     (s.Name == "CPU Package" || s.Name == "Package"))
            {
                if (s.Value > 0) power = s.Value;
            }
        }
    }

    private static GpuDevice ExtractGpu(IHardware hw)
    {
        float? load = null, temp = null, hotspot = null, coreClock = null,
               memClock = null, power = null, fan = null,
               vramUsed = null, vramTotal = null, d3d3d = null, d3dVdec = null,
               gpuCorePower = null, gpuSocPower = null;

        foreach (var s in hw.Sensors)
        {
            if (s.Value is null) continue;
            switch (s.SensorType)
            {
                case SensorType.Load:
                    if (s.Name == "GPU Core") load = s.Value;
                    else if (s.Name is "GPU D3D 3D" or "D3D 3D") d3d3d = s.Value;
                    // Video-decode engine load. NVIDIA exposes "D3D Video Decode";
                    // AMD names it "D3D Video Decode 1" (iGPU) or routes decoding
                    // through the unified "D3D Video Codec Engine" (discrete VCN).
                    // Match any such engine and keep the busiest.
                    else if (s.Name.Contains("Video Decode") || s.Name.Contains("Video Codec"))
                        d3dVdec = Math.Max(d3dVdec ?? 0f, s.Value.Value);
                    break;
                case SensorType.Temperature:
                    if (s.Name == "GPU Core") temp = s.Value;
                    else if (s.Name is "GPU Hot Spot" or "GPU Hot Spot Temperature") hotspot = s.Value;
                    else if (temp is null && s.Name == "GPU VR SoC") temp = s.Value;
                    break;
                case SensorType.Clock:
                    if (s.Name == "GPU Core") coreClock = s.Value;
                    else if (s.Name is "GPU Memory" or "GPU Memory Clock") memClock = s.Value;
                    break;
                case SensorType.Power:
                    if (s.Name is "GPU Package" or "GPU Power") power = s.Value;
                    else if (s.Name == "GPU Core") gpuCorePower = s.Value;
                    else if (s.Name == "GPU SoC") gpuSocPower = s.Value;
                    break;
                case SensorType.Control:
                    if (s.Name.Contains("GPU Fan"))
                        fan = fan.HasValue ? (fan + s.Value) / 2f : s.Value;
                    break;
                case SensorType.SmallData:
                    if (s.Name == "GPU Memory Used") vramUsed = s.Value;
                    else if (s.Name == "GPU Memory Total") vramTotal = s.Value;
                    break;
                case SensorType.Data:
                    // Some drivers report VRAM in GB (Data) instead of MB (SmallData)
                    if (s.Name == "GPU Memory Used") vramUsed = s.Value * 1024f;
                    else if (s.Name == "GPU Memory Total") vramTotal = s.Value * 1024f;
                    break;
            }
        }

        var family = hw.Identifier.ToString()
            .Split('/', StringSplitOptions.RemoveEmptyEntries)
            .FirstOrDefault(p => p.StartsWith("gpu", StringComparison.OrdinalIgnoreCase))
            ?? "gpu";

        // AMD iGPU fallback: no GPU Package/Power — sum Core + SoC instead
        if (power is null && (gpuCorePower.HasValue || gpuSocPower.HasValue))
            power = (gpuCorePower ?? 0f) + (gpuSocPower ?? 0f);

        return new GpuDevice(hw.Name, family, load, temp, hotspot, coreClock, memClock,
                             power, fan, vramUsed, vramTotal, d3d3d, d3dVdec);
    }

    private static void ExtractDisk(IHardware hw, Dictionary<string, float> diskTemps)
    {
        var id = hw.Identifier.ToString();
        if (!id.StartsWith("/nvme/") && !id.StartsWith("/hdd/") && !id.StartsWith("/ata/") &&
            !id.StartsWith("/scsi/") && !id.StartsWith("/ssd/"))
            return;

        float? highest = null;
        foreach (var s in hw.Sensors)
        {
            if (s.SensorType != SensorType.Temperature || s.Value is null) continue;
            // Exclude Warning Composite / Critical Composite threshold sensors
            if (s.Name.Contains("Warning") || s.Name.Contains("Critical")) continue;
            if (s.Value > (highest ?? float.MinValue)) highest = s.Value;
        }

        if (highest.HasValue)
            diskTemps[hw.Name] = highest.Value;
    }

    private static void ExtractRam(IHardware hw, ref float? ramTemp)
    {
        float? max = null;
        foreach (var s in hw.Sensors)
        {
            var id = s.Identifier.ToString();
            if (!id.StartsWith("/memory/") || !id.EndsWith("/temperature/0")) continue;
            if (s.Value is null) continue;
            if (s.Value > (max ?? float.MinValue)) max = s.Value;
        }
        if (max.HasValue) ramTemp = max;
    }

    private static void ExtractMotherboard(
        IHardware hw,
        List<MbFan> fans, List<MbTemp> temps, List<MbVoltage> voltages,
        ref string? mbChip)
    {
        foreach (var sub in hw.SubHardware)
        {
            if (!sub.Identifier.ToString().StartsWith("/lpc/")) continue;
            mbChip ??= sub.Name;

            foreach (var s in sub.Sensors)
            {
                if (s.Value is null) continue;
                switch (s.SensorType)
                {
                    case SensorType.Fan:
                        if (s.Value > 0) fans.Add(new MbFan(s.Name, s.Value.Value));
                        break;
                    case SensorType.Temperature:
                        if (s.Value >= 5) temps.Add(new MbTemp(s.Name, s.Value.Value));
                        break;
                    case SensorType.Voltage:
                        // Exclude generic unmapped slots — only named rails
                        if (s.Value > 0.1f && !s.Name.StartsWith("Voltage #"))
                            voltages.Add(new MbVoltage(s.Name, s.Value.Value));
                        break;
                }
            }
        }

        fans.Sort((a, b) => b.Rpm.CompareTo(a.Rpm));
    }
}
