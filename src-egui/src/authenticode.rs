#![allow(unsafe_code)]
//! Authenticode verification for the updater: is a file validly signed, and
//! by whom.
//!
//! `#![allow(unsafe_code)]`: the module is a thin FFI shim over
//! `wintrust.dll`/`crypt32.dll`; the policy (whose signature is accepted)
//! lives in `update_check.rs`.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HANDLE, HWND, INVALID_HANDLE_VALUE};
use windows::Win32::Security::Cryptography::{
    CertNameToStrW, CERT_X500_NAME_STR, X509_ASN_ENCODING,
};
use windows::Win32::Security::WinTrust::{
    WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData,
    WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_FILE_INFO,
    WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE,
};

/// The subject (X.500 string, e.g. `CN=…, O=…, C=…`) of the certificate that
/// signed `path`, when the file carries a valid, trusted Authenticode
/// signature. An unsigned file, a signature that doesn't match the file, or
/// an untrusted chain is an error.
///
/// Revocation is not checked: it needs the network at the worst moment and
/// would refuse updates for a reason the user can't act on.
pub fn verified_signer_subject(path: &Path) -> Result<String, String> {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut file = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_REVOCATION_CHECK_NONE,
        ..Default::default()
    };
    data.Anonymous.pFile = &mut file;
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let data_ptr = std::ptr::addr_of_mut!(data);
    // No window: WTD_UI_NONE, and INVALID_HANDLE_VALUE asks for no UI either.
    let hwnd = HWND(INVALID_HANDLE_VALUE.0);

    // SAFETY: `data` and `file` (and `wide`, the path they point at) outlive
    // both calls; the verify call's state data is read only before the
    // close call releases it.
    unsafe {
        let status = WinVerifyTrust(hwnd, &mut action, data_ptr.cast());
        let subject = if status == 0 {
            signer_subject((*data_ptr).hWVTStateData)
        } else {
            None
        };

        // The state data is allocated whatever the verdict.
        (*data_ptr).dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(hwnd, &mut action, data_ptr.cast());

        if status != 0 {
            return Err(format!(
                "no valid signature (WinVerifyTrust 0x{:08X})",
                status as u32
            ));
        }
        subject.ok_or_else(|| "the signer certificate could not be read".to_string())
    }
}

/// # Safety
/// `state` must be the `hWVTStateData` of a verify call not yet closed.
unsafe fn signer_subject(state: HANDLE) -> Option<String> {
    // SAFETY: every pointer is checked for null before it is read; all of
    // them belong to `state`, which the caller keeps open.
    unsafe {
        let prov_data = WTHelperProvDataFromStateData(state);
        if prov_data.is_null() {
            return None;
        }
        let signer = WTHelperGetProvSignerFromChain(prov_data, 0, false, 0);
        if signer.is_null() {
            return None;
        }
        // Index 0 is the signing certificate; the rest is its chain.
        let provider_cert = WTHelperGetProvCertFromChain(signer, 0);
        if provider_cert.is_null() || (*provider_cert).pCert.is_null() {
            return None;
        }
        let info = (*(*provider_cert).pCert).pCertInfo;
        if info.is_null() {
            return None;
        }
        let name = std::ptr::addr_of!((*info).Subject);
        let len = CertNameToStrW(X509_ASN_ENCODING, name, CERT_X500_NAME_STR, None);
        if len <= 1 {
            return None;
        }
        let mut buf = vec![0u16; len as usize];
        let written = CertNameToStrW(X509_ASN_ENCODING, name, CERT_X500_NAME_STR, Some(&mut buf));
        if written <= 1 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..written as usize - 1]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_installer_yields_its_signer() {
        // The PawnIO setup bundled in the repo carries an embedded signature.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/pawnio/PawnIO_setup.exe");
        let subject = verified_signer_subject(&path).expect("PawnIO setup is signed");
        assert!(subject.contains("namazso"), "unexpected signer: {subject}");
    }

    #[test]
    fn unsigned_file_is_rejected() {
        let path =
            std::env::temp_dir().join(format!("rigstats-unsigned-{}.exe", std::process::id()));
        std::fs::write(&path, b"MZ not really an executable").unwrap();
        let result = verified_signer_subject(&path);
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err());
    }

    fn pawnio_setup() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/pawnio/PawnIO_setup.exe")
    }

    /// The signer's subject as Windows' own `Get-AuthenticodeSignature`
    /// reads it — an oracle that shares no code with this module. `Decode`
    /// without flags keeps the certificate's order (`Subject` reverses it).
    fn powershell_subject(path: &Path) -> String {
        let script = format!(
            "(Get-AuthenticodeSignature -LiteralPath '{}').SignerCertificate.SubjectName.Decode('None')",
            path.canonicalize().unwrap().display()
        );
        let out = std::process::Command::new("powershell")
            // Inherited from a PowerShell 7 shell, it keeps Windows
            // PowerShell from loading its own modules.
            .env_remove("PSModulePath")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .expect("powershell runs");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn subject_is_the_one_windows_reports() {
        // The updater compares the installer's subject with the running
        // app's for equality, so the whole string is pinned, not a part.
        let mut files = vec![pawnio_setup()];
        // The installed app, on a machine that has it (not CI).
        let installed = Path::new(r"C:\Program Files\RIGStats\rigstats.exe");
        if installed.exists() {
            files.push(installed.to_path_buf());
        }
        for file in files {
            let ours = verified_signer_subject(&file).expect("signed");
            assert_eq!(ours, powershell_subject(&file), "{}", file.display());
        }
    }

    #[test]
    fn a_changed_signed_file_is_rejected() {
        // One byte in the code, outside the signature: the digest no longer
        // matches (TRUST_E_BAD_DIGEST).
        let mut bytes = std::fs::read(pawnio_setup()).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xFF;
        let path =
            std::env::temp_dir().join(format!("rigstats-changed-{}.exe", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let result = verified_signer_subject(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            result,
            Err("no valid signature (WinVerifyTrust 0x80096010)".to_string())
        );
    }

    #[test]
    fn a_signature_from_an_untrusted_root_is_rejected() {
        // Signed with a self-signed certificate made for this fixture and
        // deleted afterwards (CERT_E_UNTRUSTEDROOT): anyone can sign a file,
        // only a trusted chain counts.
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/authenticode/untrusted-signed.dll");
        assert_eq!(
            verified_signer_subject(&path),
            Err("no valid signature (WinVerifyTrust 0x800B0109)".to_string())
        );
    }

    #[test]
    fn a_catalog_signed_file_is_rejected() {
        // Only a signature inside the file counts; Windows' own files are
        // signed through catalogs, which are not consulted
        // (TRUST_E_NOSIGNATURE). The installer always carries its own.
        let path = Path::new(r"C:\Windows\System32\notepad.exe");
        assert_eq!(
            verified_signer_subject(path),
            Err("no valid signature (WinVerifyTrust 0x800B0100)".to_string())
        );
    }

    #[test]
    fn a_missing_file_is_rejected() {
        let path =
            std::env::temp_dir().join(format!("rigstats-missing-{}.exe", std::process::id()));
        assert!(verified_signer_subject(&path).is_err());
    }
}
