namespace SensorSidecar.Control;

/// What the user sees in Status when the PawnIO driver — through which the
/// CPU's temperature, clocks and power are read — isn't there (#507).
/// Checked once at start: LHM opens the driver at start too, so a driver
/// installed later is read after the next service start anyway.
public static class PawnIoProblems
{
    /// PawnIO's INF installs only on `NTamd64.10.0...17763` and its catalog
    /// is signed for `10_RS5` and newer (PawnIO 2.2.0 source,
    /// `PawnIO/PawnIO.inf.in`, `PawnIO/CMakeLists.txt`): Windows 10 1809.
    /// Its setup exits with 50 (ERROR_NOT_SUPPORTED) on older builds.
    public const int MinimumBuild = 17763;

    public const string ReadMoreAnchor = "#cpu-temperature-or-package-power-missing";

    /// The problem for this machine, or null when the driver is there.
    /// `windowsBuild` 0 = unknown, read as new enough.
    public static Problem? For(bool driverAvailable, int windowsBuild)
    {
        if (driverAvailable)
            return null;
        if (windowsBuild is > 0 and < MinimumBuild)
        {
            return new Problem
            {
                Id = "pawnio_windows_too_old",
                Severity = ProblemSeverity.Attention,
                Title = "CPU temperature and power need a newer Windows",
                Message = $"RIGStats reads the CPU's temperature, clocks and power through PawnIO, a driver that installs on Windows 10 version 1809 (build {MinimumBuild}) or newer. "
                    + $"This PC runs build {windowsBuild}, so those values stay empty. "
                    + "Everything else — the graphics card, drives, fans, lighting — works. Update Windows to read the CPU too.",
                Actions = [ControlProblems.ReadMore(ReadMoreAnchor)],
                Detail = $"PawnIO driver not available; Windows build {windowsBuild} < {MinimumBuild}",
            };
        }
        return new Problem
        {
            Id = "pawnio_missing",
            Severity = ProblemSeverity.Attention,
            Title = "CPU temperature and power can't be read",
            Message = "RIGStats reads the CPU's temperature, clocks and power through PawnIO, a driver its installer sets up — and it isn't there. "
                + "Everything else works. Install RIGStats again, or install PawnIO yourself (winget install namazso.PawnIO), then restart the PC.",
            Actions = [ControlProblems.ReadMore(ReadMoreAnchor), ControlProblems.Diagnostics()],
            Detail = windowsBuild > 0 ? $"PawnIO driver not available; Windows build {windowsBuild}" : "PawnIO driver not available",
        };
    }

    /// The real check: can the driver be opened, and which Windows build is this.
    public static Problem? Check() =>
        For(Providers.PawnIoModule.DriverAvailable(), Environment.OSVersion.Version.Build);
}
