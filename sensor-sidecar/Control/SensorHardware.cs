using System.Text;
using LibreHardwareMonitor.Hardware;
using SensorSidecar; // UpdateVisitor, SensorReader

namespace SensorSidecar.Control;

/// One controllable fan header, as discovered by enumerating `/lpc/`
/// `SensorType.Control` sensors (siblings of the read-only `SensorType.Fan`
/// RPM sensors `SensorReader` extracts).
public readonly record struct FanHeaderInfo(string Id, string Label, double Min, double Max);

/// A header read back from the hardware: whether it is there, whether it is
/// under software control, and the duty its PWM register holds.
public readonly record struct FanReadback(bool Found, bool Software, double? Value);

/// Everything the service needs from LibreHardwareMonitor (#374): one sample,
/// the sensor tree for diagnostics, and the fan headers. The supervisor talks
/// to it through `SensorWorkerClient`, over the `--sensor-worker` child
/// process; inside that child, and in tests, it is `LhmHardware`. One call at
/// a time — the caller serializes.
public interface ISensorHardware : IDisposable
{
    /// A fresh sample: every hardware updated, then extracted.
    SensorPayload Sample();

    /// The full sensor tree as text, for the diagnostics export.
    string SensorTree();

    IReadOnlyList<FanHeaderInfo> FanHeaders();

    /// Software control at `duty` percent (already clamped by FanProvider).
    void SetFan(string id, double duty);

    /// Back to the firmware.
    void SetFanDefault(string id);

    /// Updates the header's hardware and reads the PWM register back.
    FanReadback ReadFan(string id);

    /// Hardware no longer read because it kept crashing the worker (#374).
    IReadOnlyList<Problem> Problems => [];
}

/// `ISensorHardware` on an LHM `IComputer` — in the hardware worker process
/// over the real, opened `Computer`; in tests over a fake tree. `skip` is the
/// top-level hardware (`/gpu-nvidia/0`) the worker leaves alone because it
/// kept crashing it; `updating` hears which hardware is about to be read, so
/// the service knows what was being read when the worker died (#374).
public sealed class LhmHardware(IComputer computer, IReadOnlySet<string>? skip = null, Action<IHardware>? updating = null)
    : ISensorHardware
{
    private readonly UpdateVisitor _visitor = new();

    public SensorPayload Sample()
    {
        Update();
        return SensorReader.Extract(Visible());
    }

    /// Every hardware read, except what is skipped.
    private void Update()
    {
        foreach (var hardware in Visible())
        {
            updating?.Invoke(hardware);
            hardware.Accept(_visitor);
        }
    }

    private IEnumerable<IHardware> Visible() =>
        skip is null || skip.Count == 0
            ? computer.Hardware
            : computer.Hardware.Where(h => !skip.Contains(h.Identifier.ToString()));

    public string SensorTree()
    {
        Update();
        var lines = new StringBuilder();
        lines.AppendLine(System.Globalization.CultureInfo.InvariantCulture, $"# sensor-tree — {DateTimeOffset.UtcNow:yyyy-MM-dd HH:mm:ss}Z");
        // Data sensors are in bytes since LHM 0.9.7; older dumps (MB/GB,
        // with SmallData) have no such line — SensorTreeLoader converts them.
        lines.AppendLine("# data-unit: bytes");
        foreach (var id in skip ?? new HashSet<string>())
            lines.AppendLine(System.Globalization.CultureInfo.InvariantCulture, $"# skipped: {id} (it kept crashing the hardware worker)");
        foreach (var hw in Visible())
        {
            lines.AppendLine(System.Globalization.CultureInfo.InvariantCulture, $"HW  {hw.HardwareType,-20} id={hw.Identifier} name={hw.Name}");
            foreach (var s in hw.Sensors)
                lines.AppendLine(System.Globalization.CultureInfo.InvariantCulture, $"  S  {s.SensorType,-15} id={s.Identifier} name={s.Name} val={s.Value}");
            foreach (var sub in hw.SubHardware)
            {
                lines.AppendLine(System.Globalization.CultureInfo.InvariantCulture, $"  SUB {sub.HardwareType,-18} id={sub.Identifier} name={sub.Name}");
                foreach (var s in sub.Sensors)
                    lines.AppendLine(System.Globalization.CultureInfo.InvariantCulture, $"    S  {s.SensorType,-15} id={s.Identifier} name={s.Name} val={s.Value}");
            }
        }
        return lines.ToString();
    }

    public IReadOnlyList<FanHeaderInfo> FanHeaders() => FindControlSensors(computer)
        .Select(s => new FanHeaderInfo(s.Identifier.ToString(), s.Name, s.Control!.MinSoftwareValue, s.Control!.MaxSoftwareValue))
        .ToList();

    public void SetFan(string id, double duty) => FindControlSensor(computer, id)?.Control?.SetSoftware((float)duty);

    public void SetFanDefault(string id) => FindControlSensor(computer, id)?.Control?.SetDefault();

    public FanReadback ReadFan(string id)
    {
        if (FindControlSensor(computer, id) is not { Control: { } control } sensor)
            return new FanReadback(false, false, null);
        // ControlMode is only LHM's own bookkeeping — the PWM register itself
        // is re-read, so firmware that silently overrides the write shows.
        sensor.Hardware.Update();
        return new FanReadback(true, control.ControlMode == ControlMode.Software, sensor.Value);
    }

    public void Dispose() { }

    /// `/lpc/<chip>/0/control/<n>` sensors — the writable siblings of the
    /// read-only `/lpc/<chip>/0/fan/<n>` RPM sensors `SensorReader` extracts.
    /// GPU fan controls are not motherboard headers.
    private static IEnumerable<ISensor> FindControlSensors(IComputer computer) =>
        computer.Hardware
            .Where(hw => hw.HardwareType == HardwareType.Motherboard)
            .SelectMany(hw => hw.SubHardware)
            .SelectMany(sub => sub.Sensors)
            .Where(s => s.SensorType == SensorType.Control
                && s.Identifier.ToString().StartsWith("/lpc/", StringComparison.Ordinal)
                && s.Control is not null);

    private static ISensor? FindControlSensor(IComputer computer, string id) =>
        FindControlSensors(computer).FirstOrDefault(s => s.Identifier.ToString() == id);
}
