using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// <see cref="PipeClientVerifier.CheckImageAndSigner"/> is the pure
/// comparison logic behind steps 2–3 of client verification (image path +
/// Authenticode signer match) — see that method's doc for why the
/// P/Invoke-dependent parts (steps 1, and resolving the two thumbprints) are
/// exercised by the manual pipe smoke test instead.
/// </summary>
public class PipeClientVerifierTests
{
    private const string Expected = @"C:\Program Files\RIGStats\rigstats.exe";

    [Fact]
    public void Allows_when_path_and_signer_both_match()
    {
        var result = PipeClientVerifier.CheckImageAndSigner(Expected, Expected, "ABC123", "ABC123");
        Assert.True(result.Allowed);
    }

    [Fact]
    public void Allows_when_paths_differ_only_by_case_or_relative_segments()
    {
        var result = PipeClientVerifier.CheckImageAndSigner(
            @"C:\Program Files\RIGStats\..\RIGStats\RIGSTATS.EXE", Expected, "ABC123", "ABC123");
        Assert.True(result.Allowed);
    }

    [Fact]
    public void Denies_when_image_path_does_not_match()
    {
        var result = PipeClientVerifier.CheckImageAndSigner(
            @"C:\Users\evil\notrigstats.exe", Expected, "ABC123", "ABC123");
        Assert.False(result.Allowed);
        Assert.Contains("image path", result.Reason);
    }

    [Fact]
    public void Denies_when_signer_thumbprints_differ()
    {
        var result = PipeClientVerifier.CheckImageAndSigner(Expected, Expected, "ABC123", "DEF456");
        Assert.False(result.Allowed);
        Assert.Contains("signer", result.Reason);
    }

    [Theory]
    [InlineData(null, "ABC123")]
    [InlineData("ABC123", null)]
    [InlineData(null, null)]
    public void Denies_when_either_signature_could_not_be_read(string? own, string? client)
    {
        var result = PipeClientVerifier.CheckImageAndSigner(Expected, Expected, own, client);
        Assert.False(result.Allowed);
    }
}
