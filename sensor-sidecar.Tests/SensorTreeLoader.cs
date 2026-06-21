using System.Globalization;
using LibreHardwareMonitor.Hardware;
using NSubstitute;

namespace SensorSidecar.Tests;

/// <summary>
/// Rebuilds a mocked LibreHardwareMonitor hardware tree (an <see cref="IComputer"/>)
/// from the <c>sensor-tree.txt</c> dump that the sidecar writes at startup and that
/// ships inside the diagnostics ZIP. This lets real-world sensor layouts become
/// regression fixtures for <see cref="SensorReader.Extract"/>.
///
/// Line format (see SensorWorker.WriteSensorTree):
/// <code>
/// HW  &lt;HardwareType&gt;    id=&lt;identifier&gt; name=&lt;name&gt;
///   S  &lt;SensorType&gt;     id=&lt;identifier&gt; name=&lt;name&gt; val=&lt;value&gt;
///   SUB &lt;HardwareType&gt;   id=&lt;identifier&gt; name=&lt;name&gt;
///     S  &lt;SensorType&gt;   id=&lt;identifier&gt; name=&lt;name&gt; val=&lt;value&gt;
/// </code>
/// Parsing is intentionally tolerant: unknown enum values and malformed lines are
/// skipped rather than throwing, so a newer LHM dump never breaks the loader.
/// </summary>
public static class SensorTreeLoader
{
    public static IComputer LoadFile(string path) => Load(File.ReadAllLines(path));

    public static IComputer Load(IEnumerable<string> lines)
    {
        var hardware = new List<IHardware>();
        List<ISensor>? currentHwSensors = null;
        List<IHardware>? currentHwSub = null;
        List<ISensor>? currentSubSensors = null;

        foreach (var raw in lines)
        {
            if (string.IsNullOrWhiteSpace(raw) || raw.TrimStart().StartsWith('#'))
                continue;

            var indent = raw.Length - raw.TrimStart().Length;
            var trimmed = raw.TrimStart();

            if (trimmed.StartsWith("HW ", StringComparison.Ordinal))
            {
                var (type, id, name, _) = ParseFields(trimmed, "HW");
                if (!Enum.TryParse<HardwareType>(type, out var ht))
                    continue;
                currentHwSensors = [];
                currentHwSub = [];
                currentSubSensors = null;
                hardware.Add(BuildHardware(ht, name, id, currentHwSensors, currentHwSub));
            }
            else if (trimmed.StartsWith("SUB ", StringComparison.Ordinal))
            {
                if (currentHwSub is null)
                    continue;
                var (type, id, name, _) = ParseFields(trimmed, "SUB");
                Enum.TryParse<HardwareType>(type, out var ht);
                currentSubSensors = [];
                currentHwSub.Add(BuildHardware(ht, name, id, currentSubSensors, []));
            }
            else if (trimmed.StartsWith("S ", StringComparison.Ordinal))
            {
                var (type, id, name, val) = ParseFields(trimmed, "S");
                if (!Enum.TryParse<SensorType>(type, out var st))
                    continue;
                var sensor = BuildSensor(st, name, id, ParseValue(val));
                // 4+ leading spaces → belongs to the current SUB; otherwise to the HW.
                if (indent >= 4 && currentSubSensors is not null)
                    currentSubSensors.Add(sensor);
                else
                    currentHwSensors?.Add(sensor);
            }
        }

        var computer = Substitute.For<IComputer>();
        computer.Hardware.Returns(hardware.ToArray());
        return computer;
    }

    private static (string type, string id, string name, string? val) ParseFields(
        string trimmed, string kind)
    {
        // Strip the leading kind word, then split on the " id=" / " name=" / " val=" markers.
        var rest = trimmed[kind.Length..].TrimStart();
        var idIdx = rest.IndexOf("id=", StringComparison.Ordinal);
        if (idIdx < 0)
            return (rest.Trim(), "", "", null);

        var type = rest[..idIdx].Trim();
        var afterId = rest[(idIdx + 3)..];

        var nameMarker = afterId.IndexOf(" name=", StringComparison.Ordinal);
        if (nameMarker < 0)
            return (type, afterId.Trim(), "", null);

        var id = afterId[..nameMarker].Trim();
        var afterName = afterId[(nameMarker + " name=".Length)..];

        var valMarker = afterName.IndexOf(" val=", StringComparison.Ordinal);
        if (valMarker < 0)
            return (type, id, afterName.Trim(), null);

        var name = afterName[..valMarker].Trim();
        var val = afterName[(valMarker + " val=".Length)..].Trim();
        return (type, id, name, val);
    }

    private static float? ParseValue(string? val)
    {
        if (string.IsNullOrWhiteSpace(val))
            return null;
        // Accept both '.' and ',' decimal separators (sidecar culture is not guaranteed).
        var normalized = val.Replace(',', '.');
        return float.TryParse(normalized, NumberStyles.Float, CultureInfo.InvariantCulture, out var f)
            ? f
            : null;
    }

    private static Identifier ParseIdentifier(string id) =>
        new(id.Split('/', StringSplitOptions.RemoveEmptyEntries));

    private static IHardware BuildHardware(
        HardwareType type, string name, string id, List<ISensor> sensors, List<IHardware> sub)
    {
        var hw = Substitute.For<IHardware>();
        hw.HardwareType.Returns(type);
        hw.Name.Returns(name);
        hw.Identifier.Returns(ParseIdentifier(id));
        // Return the live lists so sensors/sub added after construction are visible.
        hw.Sensors.Returns(_ => sensors.ToArray());
        hw.SubHardware.Returns(_ => sub.ToArray());
        return hw;
    }

    private static ISensor BuildSensor(SensorType type, string name, string id, float? value)
    {
        var s = Substitute.For<ISensor>();
        s.SensorType.Returns(type);
        s.Name.Returns(name);
        s.Value.Returns(value);
        s.Identifier.Returns(ParseIdentifier(id));
        return s;
    }
}
