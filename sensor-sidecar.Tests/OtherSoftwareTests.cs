using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <see cref="OtherSoftware"/>: which running program takes which domain
/// (#193), by process name only.
public class OtherSoftwareTests
{
    private static Func<string, bool> Running(params string[] names) =>
        name => names.Contains(name, StringComparer.OrdinalIgnoreCase);

    [Fact]
    public void Nothing_running_takes_nothing()
    {
        Assert.Null(OtherSoftware.Conflict("aura", Running()));
        Assert.Null(OtherSoftware.Conflict("fan", Running()));
    }

    [Fact]
    public void Armoury_crates_fan_service_takes_only_the_fans()
    {
        var running = Running("AsusFanControlService");

        Assert.Equal("Armoury Crate (AsusFanControlService)", OtherSoftware.Conflict("fan", running));
        Assert.Null(OtherSoftware.Conflict("aura", running));
    }

    [Fact]
    public void Lighting_names_the_service_before_the_app_before_openrgb()
    {
        Assert.Equal("Armoury Crate (LightingService)", OtherSoftware.Conflict("aura", Running("OpenRGB", "ArmouryCrate", "LightingService")));
        Assert.Equal("Armoury Crate", OtherSoftware.Conflict("aura", Running("OpenRGB", "ArmouryCrate")));
        Assert.Equal("OpenRGB", OtherSoftware.Conflict("aura", Running("OpenRGB")));
        Assert.Null(OtherSoftware.Conflict("fan", Running("OpenRGB", "ArmouryCrate", "LightingService")));
    }

    [Fact]
    public void The_table_has_a_distinct_process_per_row_and_no_exe_suffix()
    {
        Assert.Equal(OtherSoftware.Known.Count, OtherSoftware.Known.Select(p => p.Process).Distinct().Count());
        Assert.All(OtherSoftware.Known, p => Assert.DoesNotContain(".", p.Process));
        Assert.All(OtherSoftware.Known, p => Assert.True(p.Domain is "fan" or "aura", p.Domain));
    }

    [Fact]
    public void The_start_up_line_lists_known_programs_and_other_asus_processes()
    {
        var line = OtherSoftware.Summary(["explorer", "AsusFanControlService", "ArmouryCrateService", "AsusCertService", "LightingService", "svchost"]);

        Assert.Equal(
            "Other hardware software running: Armoury Crate (LightingService) → lighting, Armoury Crate (AsusFanControlService) → fan control; "
            + "also seen: ArmouryCrateService, AsusCertService",
            line);
        Assert.Equal("Other hardware software running: none of the known ones", OtherSoftware.Summary(["explorer", "svchost"]));
    }

    [Fact]
    public void The_fan_problem_sends_armoury_crate_users_to_the_uninstall_tool()
    {
        var problem = ControlProblems.FansTakenBy("Armoury Crate (AsusFanControlService)");

        Assert.Equal("fan_conflict", problem.Id);
        Assert.Equal(ProblemSeverity.Attention, problem.Severity);
        Assert.Equal("Armoury Crate controls the fans", problem.Title);
        Assert.Contains("AsusFanControlService", problem.Message);
        Assert.Contains("everything else keeps working", problem.Message);
        Assert.Contains("uninstall tool", problem.Message);
        Assert.Equal(["ASUS uninstall tool", "Read more"], problem.Actions.Select(a => a.Label));
        Assert.All(problem.Actions, a => Assert.StartsWith("https://", a.Target));
    }

    [Fact]
    public void Another_fan_program_is_asked_to_close_instead()
    {
        var problem = ControlProblems.FansTakenBy("Some Fan Tool");

        Assert.Equal("Some Fan Tool controls the fans", problem.Title);
        Assert.Contains("Close Some Fan Tool", problem.Message);
        Assert.Equal(["Read more"], problem.Actions.Select(a => a.Label));
    }

    [Fact]
    public void The_lighting_problem_offers_the_uninstall_tool_only_for_armoury_crate()
    {
        Assert.Equal(["ASUS uninstall tool", "Read more"],
            ControlProblems.LightingTakenBy("Armoury Crate (LightingService)").Actions.Select(a => a.Label));
        Assert.Equal(["Read more"],
            ControlProblems.LightingTakenBy("OpenRGB").Actions.Select(a => a.Label));
    }
}
