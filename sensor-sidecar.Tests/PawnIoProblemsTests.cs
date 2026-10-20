using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// What Status says when the PawnIO driver is missing (#507): nothing when
/// it is there, "needs a newer Windows" below build 17763 (a beta tester's
/// Windows 10 1709, build 16299), otherwise how to install it.
/// </summary>
public class PawnIoProblemsTests
{
    [Theory]
    [InlineData(16299)]
    [InlineData(26100)]
    [InlineData(0)]
    public void No_problem_when_the_driver_is_there(int build) =>
        Assert.Null(PawnIoProblems.For(driverAvailable: true, build));

    [Theory]
    [InlineData(16299)] // 1709, seen in a beta tester's diagnostics
    [InlineData(17134)] // 1803
    [InlineData(17762)]
    public void Below_1809_says_windows_is_too_old(int build)
    {
        var p = PawnIoProblems.For(driverAvailable: false, build)!;
        Assert.Equal("pawnio_windows_too_old", p.Id);
        Assert.Contains("1809", p.Message);
        Assert.Contains($"build {build}", p.Message);
        Assert.Contains(p.Actions, a => a.Target!.EndsWith(PawnIoProblems.ReadMoreAnchor, StringComparison.Ordinal));
    }

    [Theory]
    [InlineData(17763)] // 1809, the first build PawnIO installs on
    [InlineData(19045)]
    [InlineData(26100)]
    [InlineData(0)] // unknown build: don't blame Windows
    public void From_1809_says_how_to_install_it(int build)
    {
        var p = PawnIoProblems.For(driverAvailable: false, build)!;
        Assert.Equal("pawnio_missing", p.Id);
        Assert.Contains("winget install namazso.PawnIO", p.Message);
        Assert.Contains(p.Actions, a => a.Kind == ProblemActionKind.CollectDiagnostics);
    }
}
