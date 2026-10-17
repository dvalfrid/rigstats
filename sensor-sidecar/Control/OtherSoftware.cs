using System.Diagnostics;

namespace SensorSidecar.Control;

/// Other programs that write the same hardware RIGStats controls (#193).
/// Two programs setting the same fan header or the same LEDs fight over
/// every change, so a domain another program owns is reported as taken
/// (`CapabilitySet.Problem`) and its writes are skipped — RIGStats yields,
/// never fights. Detection is by process name only: no service is queried
/// or touched, and nothing is stopped from here; the problem's action links
/// to ASUS's own uninstall tool instead.
///
/// `Verified` says whether the program was seen writing that hardware on a
/// real machine. `AsusFanControlService` is Armoury Crate's fan service
/// (Fan Xpert 4) — a hypothesis from how Armoury Crate installs; the owner's
/// rig has no Armoury Crate, so the service log's start-up line ("Other
/// hardware software running: …") in users' diagnostics is what confirms or
/// corrects the table.
public static class OtherSoftware
{
    /// One program: its process name (without `.exe`), the name the user
    /// sees (the program first, its component in parentheses — the
    /// problem's title uses the part before " ("), and the domain it writes.
    public sealed record Program(string Process, string Name, string Domain, bool Verified);

    /// In order of preference for the name shown when several run at once.
    public static readonly IReadOnlyList<Program> Known =
    [
        new("LightingService", "Armoury Crate (LightingService)", "aura", Verified: true),
        new("ArmouryCrate", "Armoury Crate", "aura", Verified: true),
        new("OpenRGB", "OpenRGB", "aura", Verified: true),
        new("AsusFanControlService", "Armoury Crate (AsusFanControlService)", "fan", Verified: false),
    ];

    /// What holds the lighting besides us, or null.
    public static string? LightingTaken() => Conflict("aura", IsRunning);

    /// What sets the fans besides us, or null.
    public static string? FansTaken() => Conflict("fan", IsRunning);

    /// The name of the first known program for `domain` that `running`
    /// reports, or null.
    public static string? Conflict(string domain, Func<string, bool> running) =>
        Known.FirstOrDefault(p => p.Domain == domain && running(p.Process))?.Name;

    /// One line for the service log at start: every known program that is
    /// running, plus any other process whose name says ASUS, Armoury or Aura —
    /// the evidence for which component owns what on boards we don't have.
    public static string Summary(IEnumerable<string> processes)
    {
        var running = processes.Distinct(StringComparer.OrdinalIgnoreCase).ToList();
        var known = Known
            .Where(p => running.Contains(p.Process, StringComparer.OrdinalIgnoreCase))
            .Select(p => $"{p.Name} → {ControlProblems.Name(p.Domain).ToLowerInvariant()}")
            .ToList();
        var vendor = running
            .Where(name => Known.All(p => !string.Equals(p.Process, name, StringComparison.OrdinalIgnoreCase)))
            .Where(name => name.Contains("asus", StringComparison.OrdinalIgnoreCase)
                || name.Contains("armoury", StringComparison.OrdinalIgnoreCase)
                || name.Contains("aura", StringComparison.OrdinalIgnoreCase))
            .Order(StringComparer.OrdinalIgnoreCase)
            .ToList();
        var parts = new List<string>();
        if (known.Count > 0)
            parts.Add(string.Join(", ", known));
        if (vendor.Count > 0)
            parts.Add($"also seen: {string.Join(", ", vendor)}");
        return parts.Count == 0
            ? "Other hardware software running: none of the known ones"
            : $"Other hardware software running: {string.Join("; ", parts)}";
    }

    /// `Summary` of this machine's processes, for `Program.cs`'s start-up task.
    public static string Summary()
    {
        try
        {
            return Summary(Process.GetProcesses().Select(p => p.ProcessName));
        }
        catch (Exception e)
        {
            return $"Other hardware software running: could not list processes ({e.Message})";
        }
    }

    private static bool IsRunning(string process)
    {
        try
        {
            return Process.GetProcessesByName(process).Length > 0;
        }
        catch
        {
            return false; // Can't tell — don't block a domain over it.
        }
    }
}
