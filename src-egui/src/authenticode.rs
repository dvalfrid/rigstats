#![allow(unsafe_code)]
//! Authenticode verification for the updater: is a file validly signed, and
//! by whom.
//!
//! `#![allow(unsafe_code)]`: the module is a thin FFI shim over
//! `wintrust.dll`/`crypt32.dll`; the policy (whose signature is accepted)
//! lives in `update_check.rs`.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use winapi::ctypes::c_void;
use winapi::shared::minwindef::{BOOL, DWORD};
use winapi::shared::windef::HWND;
use winapi::um::handleapi::INVALID_HANDLE_VALUE;
use winapi::um::softpub::WINTRUST_ACTION_GENERIC_VERIFY_V2;
use winapi::um::wincrypt::{CertNameToStrW, CERT_X500_NAME_STR, PCCERT_CONTEXT, X509_ASN_ENCODING};
use winapi::um::winnt::HANDLE;
use winapi::um::wintrust::{
    WinVerifyTrust, WINTRUST_DATA, WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE,
    WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
};

/// The head of `CRYPT_PROVIDER_CERT` — only the certificate is read.
#[repr(C)]
struct ProviderCert {
    cb_struct: DWORD,
    cert: PCCERT_CONTEXT,
}

// Not in the `winapi` crate.
#[link(name = "wintrust")]
extern "system" {
    fn WTHelperProvDataFromStateData(state: HANDLE) -> *mut c_void;
    fn WTHelperGetProvSignerFromChain(
        prov_data: *mut c_void,
        signer_index: DWORD,
        counter_signer: BOOL,
        counter_signer_index: DWORD,
    ) -> *mut c_void;
    fn WTHelperGetProvCertFromChain(signer: *mut c_void, cert_index: DWORD) -> *mut ProviderCert;
}

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

    unsafe {
        let mut file: WINTRUST_FILE_INFO = std::mem::zeroed();
        file.cbStruct = std::mem::size_of::<WINTRUST_FILE_INFO>() as DWORD;
        file.pcwszFilePath = wide.as_ptr();

        let mut data: WINTRUST_DATA = std::mem::zeroed();
        data.cbStruct = std::mem::size_of::<WINTRUST_DATA>() as DWORD;
        data.dwUIChoice = WTD_UI_NONE;
        data.fdwRevocationChecks = WTD_REVOKE_NONE;
        data.dwUnionChoice = WTD_CHOICE_FILE;
        *data.u.pFile_mut() = &mut file;
        data.dwStateAction = WTD_STATEACTION_VERIFY;
        data.dwProvFlags = WTD_REVOCATION_CHECK_NONE;

        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let data_ptr = std::ptr::addr_of_mut!(data);
        let status = WinVerifyTrust(INVALID_HANDLE_VALUE as HWND, &mut action, data_ptr.cast());
        let subject = if status == 0 {
            signer_subject((*data_ptr).hWVTStateData)
        } else {
            None
        };

        // The state data is allocated whatever the verdict.
        (*data_ptr).dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(INVALID_HANDLE_VALUE as HWND, &mut action, data_ptr.cast());

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
    let prov_data = WTHelperProvDataFromStateData(state);
    if prov_data.is_null() {
        return None;
    }
    let signer = WTHelperGetProvSignerFromChain(prov_data, 0, 0, 0);
    if signer.is_null() {
        return None;
    }
    // Index 0 is the signing certificate; the rest is its chain.
    let provider_cert = WTHelperGetProvCertFromChain(signer, 0);
    if provider_cert.is_null() || (*provider_cert).cert.is_null() {
        return None;
    }
    let info = (*(*provider_cert).cert).pCertInfo;
    if info.is_null() {
        return None;
    }
    let name = std::ptr::addr_of_mut!((*info).Subject);
    let len = CertNameToStrW(
        X509_ASN_ENCODING,
        name,
        CERT_X500_NAME_STR,
        std::ptr::null_mut(),
        0,
    );
    if len <= 1 {
        return None;
    }
    let mut buf = vec![0u16; len as usize];
    let written = CertNameToStrW(
        X509_ASN_ENCODING,
        name,
        CERT_X500_NAME_STR,
        buf.as_mut_ptr(),
        len,
    );
    if written <= 1 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..written as usize - 1]))
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
}
