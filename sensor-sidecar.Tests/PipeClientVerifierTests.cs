using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// <see cref="PipeClientVerifier.CheckImageAndSigner"/> is the pure
/// comparison logic behind steps 2–3 of client verification (image path +
/// Authenticode signer match) — see that method's doc for why the
/// P/Invoke-dependent parts (steps 1, and resolving the two thumbprints) are
/// exercised by the manual pipe smoke test instead. The signer reader itself
/// (<see cref="PipeClientVerifier.SignerThumbprint"/>) runs against real
/// files below — a reader that couldn't read signed executables shipped in
/// 1.42.0 and refused every Control Center client.
/// </summary>
public class PipeClientVerifierTests
{
    [Fact]
    public void Reads_the_signer_of_a_signed_executable()
    {
        // The process running the tests (testhost.exe / dotnet.exe) is
        // Authenticode-signed by Microsoft, here and on CI.
        var thumbprint = PipeClientVerifier.SignerThumbprint(Environment.ProcessPath);

        Assert.False(string.IsNullOrEmpty(thumbprint), $"no signer read from {Environment.ProcessPath}");
        Assert.Equal(40, thumbprint!.Length); // SHA-1 hex
    }

    [Fact]
    public void An_unsigned_or_missing_file_has_no_signer()
    {
        var unsigned = Path.Combine(Path.GetTempPath(), $"rigstats-unsigned-{Guid.NewGuid():N}.exe");
        File.WriteAllBytes(unsigned, [0x4D, 0x5A, 0, 0]);
        try
        {
            Assert.Null(PipeClientVerifier.SignerThumbprint(unsigned));
        }
        finally
        {
            File.Delete(unsigned);
        }
        Assert.Null(PipeClientVerifier.SignerThumbprint(@"C:\does\not\exist.exe"));
        Assert.Null(PipeClientVerifier.SignerThumbprint(null));
    }

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
