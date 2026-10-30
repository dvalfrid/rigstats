# Authenticode fixtures

Files for the tests in `src/authenticode.rs` and `src/update_check.rs` (#702).

| File | What it is | Expected |
|---|---|---|
| `untrusted-signed.dll` | A 3.5 KB .NET class library signed (SHA-256) with a self-signed code-signing certificate, `CN=RIGStats test signer (untrusted), O=RIGStats tests`, valid for 100 years. The certificate and its key were deleted right after signing, so nothing can sign with it again. | `WinVerifyTrust` refuses it: `CERT_E_UNTRUSTEDROOT` (0x800B0109) |

Made on 2026-10-30 in PowerShell 7:

```powershell
Add-Type -TypeDefinition 'public static class RigStatsAuthenticodeFixture { public static int Answer() { return 42; } }' -OutputAssembly untrusted-signed.dll -OutputType Library
$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=RIGStats test signer (untrusted), O=RIGStats tests' -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddYears(100) -KeyExportPolicy NonExportable
Set-AuthenticodeSignature -FilePath untrusted-signed.dll -Certificate $cert -HashAlgorithm SHA256
Remove-Item "Cert:\CurrentUser\My\$($cert.Thumbprint)" -DeleteKey
```

The other cases need no fixture of their own:
- **Validly signed:** `build/pawnio/PawnIO_setup.exe`.
- **Changed after signing:** a copy of it with one byte flipped.
- **Catalog-signed only:** `C:\Windows\System32\notepad.exe`.
