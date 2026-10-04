using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;

namespace SensorSidecar.Control;

/// Verifies a connecting control-pipe client is genuinely the installed
/// RIGStats app, not any other process that happened to find the pipe name.
/// The control pipe is a write path into a LocalSystem service, so this is a
/// real security boundary, not a convenience check (see
/// `docs/control-architecture.md`, "Security"):
///
/// 1. PID of the connected client (`GetNamedPipeClientProcessId`).
/// 2. That process's own image path must equal the installed `rigstats.exe`
///    sitting next to this service's own exe.
/// 3. Both exes' Authenticode signer must match — works for dev (self-signed)
///    and release (real cert) builds without hardcoding a specific cert.
///
/// Skipped only in debug builds, per the doc.
public interface IPipeClientVerifier
{
    VerifyResult Verify(NamedPipeServerStream pipe);
}

public readonly record struct VerifyResult(bool Allowed, string? Reason)
{
    public static VerifyResult Allow() => new(true, null);
    public static VerifyResult Deny(string reason) => new(false, reason);
}

public sealed class PipeClientVerifier : IPipeClientVerifier
{
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetNamedPipeClientProcessId(SafeHandle pipe, out uint clientProcessId);

    public VerifyResult Verify(NamedPipeServerStream pipe)
    {
#if DEBUG
        return VerifyResult.Allow();
#else
        if (!GetNamedPipeClientProcessId(pipe.SafePipeHandle, out var pid))
            return VerifyResult.Deny("could not determine client process id.");

        string clientImagePath;
        try
        {
            using var process = System.Diagnostics.Process.GetProcessById((int)pid);
            clientImagePath = process.MainModule?.FileName
                ?? throw new InvalidOperationException("no main module.");
        }
        catch (Exception e)
        {
            return VerifyResult.Deny($"could not resolve client image path: {e.Message}");
        }

        var expectedImagePath = Path.Combine(AppContext.BaseDirectory, "rigstats.exe");
        var ownSignerThumbprint = SignerThumbprint(Environment.ProcessPath);
        var clientSignerThumbprint = SignerThumbprint(clientImagePath);

        return CheckImageAndSigner(clientImagePath, expectedImagePath, ownSignerThumbprint, clientSignerThumbprint);
#endif
    }

    /// The pure comparison logic (steps 2–3 of the class doc), split out from
    /// <see cref="Verify"/> so it's unit-testable without real P/Invoke calls,
    /// a real second process, or a real Authenticode-signed file — those are
    /// exercised by the manual pipe smoke test instead (see the #187 plan).
    internal static VerifyResult CheckImageAndSigner(
        string clientImagePath,
        string expectedImagePath,
        string? ownSignerThumbprint,
        string? clientSignerThumbprint)
    {
        if (!string.Equals(
                Path.GetFullPath(clientImagePath),
                Path.GetFullPath(expectedImagePath),
                StringComparison.OrdinalIgnoreCase))
        {
            return VerifyResult.Deny($"client image path '{clientImagePath}' is not the installed rigstats.exe.");
        }

        if (ownSignerThumbprint is null || clientSignerThumbprint is null)
            return VerifyResult.Deny("could not read Authenticode signature.");
        if (!string.Equals(ownSignerThumbprint, clientSignerThumbprint, StringComparison.OrdinalIgnoreCase))
            return VerifyResult.Deny("client Authenticode signer does not match this service's signer.");

        return VerifyResult.Allow();
    }

    /// The thumbprint of the certificate that Authenticode-signed this
    /// executable, or null when it isn't signed (or can't be read). Built in
    /// every configuration so the tests exercise the real reader — a reader
    /// that couldn't read signed executables shipped in 1.42.0 and refused
    /// every Control Center client.
    internal static string? SignerThumbprint(string? imagePath)
    {
        if (imagePath is null)
            return null;
        try
        {
            // CreateFromSignedFile is the one .NET API that reads the signer
            // embedded in a PE file. SYSLIB0057 obsoletes it in favour of
            // X509CertificateLoader, but that only loads certificate files
            // (.cer/.pem/.pfx) and throws on a signed .exe — so it is not a
            // replacement here.
#pragma warning disable SYSLIB0057
            using var cert = X509Certificate.CreateFromSignedFile(imagePath);
#pragma warning restore SYSLIB0057
            return cert.GetCertHashString();
        }
        catch (CryptographicException)
        {
            return null; // not signed
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return null;
        }
    }
}
