using NSubstitute;
using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// #385: handing control back is logged — what, and why — and a provider that
/// fails is logged without keeping the others from their turn.
public class SafetyGuardTests
{
    private static IControlProvider Provider(string domain, Func<string?> release)
    {
        var provider = Substitute.For<IControlProvider>();
        provider.Domain.Returns(domain);
        provider.ReleaseToFirmware().Returns(_ => release());
        return provider;
    }

    [Fact]
    public void Every_provider_gets_its_turn_even_after_one_fails()
    {
        var fan = Provider("fan", () => throw new IOException("Super I/O gone"));
        var cpu = Provider("cpu_limit", () => "the BIOS limits");

        new SafetyGuard([fan, cpu], dryRun: false).ReleaseAllToFirmware();

        fan.Received(1).ReleaseToFirmware();
        cpu.Received(1).ReleaseToFirmware();
    }

    [Fact]
    public void The_line_says_why_and_what_each_domain_handed_back()
    {
        Assert.Equal(
            "[rigstats-control] Handed back to the firmware because the service stops: fan: 2 headers to the BIOS (Fan #1, Fan #2); cpu_limit: the BIOS limits (PPT 162 W, TDC 120 A, EDC 180 A).",
            SafetyGuard.Line(SafetyGuard.ServiceStops, dryRun: false,
                ["fan: 2 headers to the BIOS (Fan #1, Fan #2)", "cpu_limit: the BIOS limits (PPT 162 W, TDC 120 A, EDC 180 A)"]));
    }

    [Fact]
    public void Nothing_handed_back_is_no_line_and_dry_run_says_so()
    {
        Assert.Null(SafetyGuard.Line(SafetyGuard.ServiceStops, dryRun: false, []));
        Assert.Contains("(dry-run: nothing written)", SafetyGuard.Line(SafetyGuard.AskedFromControlCenter, dryRun: true, ["fan: 1 header to the BIOS (Fan #1)"]));
    }
}
