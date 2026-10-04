use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Set at build time to test the updater against another manifest.
const CUSTOM_MANIFEST_URL: Option<&str> = option_env!("RIGSTATS_UPDATE_URL");

const LATEST_JSON_URL: &str = match CUSTOM_MANIFEST_URL {
    Some(url) => url,
    None => "https://github.com/dvalfrid/rigstats/releases/latest/download/latest.json",
};

/// The only place an installer is downloaded from (unless the manifest
/// itself was replaced at build time).
const RELEASE_URL_PREFIX: &str = "https://github.com/dvalfrid/rigstats/releases/download/";

/// Far above any real installer; stops a download that never ends.
const MAX_INSTALLER_BYTES: u64 = 300 * 1024 * 1024;

const FILE_SHARE_READ: u32 = 0x1;

/// Full version history bundled at compile time from CHANGELOG.md.
pub const BUNDLED_CHANGELOG: &str = include_str!("../../CHANGELOG.md");

#[derive(Debug, Deserialize)]
struct LatestManifest {
    version: String,
    notes: Option<String>,
    platforms: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub url: String,
    /// SHA-256 of the installer, lower-case hex, from the manifest.
    pub sha256: String,
}

/// Result of a version check.
pub enum CheckResult {
    /// Already on the latest version.
    UpToDate,
    /// A newer version is available.
    UpdateAvailable(UpdateInfo),
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(30))
        .build()
}

/// Fetch `latest.json` and compare against the current build version.
pub fn check() -> Result<CheckResult, String> {
    let resp = agent()
        .get(LATEST_JSON_URL)
        .call()
        .map_err(|e| format!("HTTP error: {e}"))?;

    let manifest: LatestManifest = resp
        .into_json()
        .map_err(|e| format!("JSON parse error: {e}"))?;

    if !is_newer(&manifest.version, env!("CARGO_PKG_VERSION")) {
        return Ok(CheckResult::UpToDate);
    }
    update_info(manifest, CUSTOM_MANIFEST_URL.is_some()).map(CheckResult::UpdateAvailable)
}

/// The installer a manifest points at. A manifest without a checksum, or
/// with a download link outside this project's releases, is refused.
fn update_info(manifest: LatestManifest, any_url: bool) -> Result<UpdateInfo, String> {
    let platform = manifest
        .platforms
        .get("windows-x86_64")
        .ok_or_else(|| "No windows-x86_64 entry in manifest".to_string())?;
    let field = |name: &str| platform.get(name).and_then(serde_json::Value::as_str);

    let url = field("url").ok_or_else(|| "No windows-x86_64 URL in manifest".to_string())?;
    if !any_url && !is_release_url(url) {
        return Err("The update manifest points outside the RIGStats releases".to_string());
    }
    let sha256 = field("sha256")
        .map(str::to_ascii_lowercase)
        .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| "No installer checksum in manifest".to_string())?;

    Ok(UpdateInfo {
        version: manifest.version,
        notes: manifest.notes.unwrap_or_default(),
        url: url.to_owned(),
        sha256,
    })
}

/// A plain file path below this project's GitHub releases — nothing that
/// could be normalised to somewhere else (`..`, escapes, a query).
fn is_release_url(url: &str) -> bool {
    url.strip_prefix(RELEASE_URL_PREFIX).is_some_and(|rest| {
        !rest.is_empty()
            && !rest.contains("..")
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
    })
}

/// A downloaded installer that passed every check: it matches the
/// manifest's checksum and carries a valid signature from this app's own
/// publisher. The file stays open without write or delete sharing for as
/// long as this value lives, so it can't be swapped before it is launched.
pub struct VerifiedInstaller {
    path: PathBuf,
    sha256: String,
    lock: std::fs::File,
}

impl VerifiedInstaller {
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn check_hash(&self) -> Result<(), String> {
        let mut file = &self.lock;
        file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("Read error: {e}"))?;
        let mut context = ring::digest::Context::new(&ring::digest::SHA256);
        let mut buf = [0u8; 65536];
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| format!("Read error: {e}"))?;
            if n == 0 {
                break;
            }
            context.update(&buf[..n]);
        }
        if hex(context.finish().as_ref()) == self.sha256 {
            Ok(())
        } else {
            Err("Update refused: the download does not match the published checksum".to_string())
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads and verifies the installer for `info`, calling
/// `on_progress(downloaded, total_or_0)` periodically. Nothing is left on
/// disk when a check fails.
pub fn download(
    info: &UpdateInfo,
    on_progress: impl Fn(u64, u64),
) -> Result<VerifiedInstaller, String> {
    let dir = fresh_download_dir()?;
    let path = dir.join("rigstats-setup.exe");
    let result = download_to(&info.url, &path, on_progress)
        .and_then(|()| lock_and_verify(path, &info.sha256));
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

/// An empty directory with an unguessable name, so the file is created by
/// this download and nothing else; earlier downloads are cleared out.
fn fresh_download_dir() -> Result<PathBuf, String> {
    use ring::rand::SecureRandom;

    let base = std::env::temp_dir().join("rigstats-update");
    let _ = std::fs::remove_dir_all(&base);
    let mut random = [0u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut random)
        .map_err(|_| "No random numbers available".to_string())?;
    let dir = base.join(hex(&random));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create folder: {e}"))?;
    Ok(dir)
}

fn download_to(url: &str, dest: &Path, on_progress: impl Fn(u64, u64)) -> Result<(), String> {
    let resp = agent()
        .get(url)
        .call()
        .map_err(|e| format!("Download HTTP error: {e}"))?;

    let total: u64 = resp
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let mut reader = resp.into_reader();
    // Readable but not writable by anyone else while it is being written.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(FILE_SHARE_READ)
        .open(dest)
        .map_err(|e| format!("Cannot create file: {e}"))?;

    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Read error: {e}"))?;
        if n == 0 {
            break;
        }
        downloaded += n as u64;
        if downloaded > MAX_INSTALLER_BYTES {
            return Err("Download refused: the installer is too large".to_string());
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Write error: {e}"))?;
        on_progress(downloaded, total);
    }
    file.flush().map_err(|e| format!("Write error: {e}"))
}

/// Re-opens the finished download read-only, sharing read access only —
/// which fails if anyone holds it open for writing, and from then on keeps
/// anyone from writing, renaming or deleting it — and checks it through
/// that handle.
fn lock_and_verify(path: PathBuf, sha256: &str) -> Result<VerifiedInstaller, String> {
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&path)
        .map_err(|e| format!("Cannot open the download: {e}"))?;
    let installer = VerifiedInstaller {
        path,
        sha256: sha256.to_owned(),
        lock,
    };
    installer.check_hash()?;

    let signer = crate::authenticode::verified_signer_subject(&installer.path)
        .map_err(|e| format!("Update refused: {e}"))?;
    let own = std::env::current_exe()
        .ok()
        .and_then(|exe| crate::authenticode::verified_signer_subject(&exe).ok());
    if !signer_accepted(own.as_deref(), &signer) {
        return Err(format!(
            "Update refused: the installer is signed by \"{signer}\", not by the publisher of this app"
        ));
    }
    Ok(installer)
}

/// The installer must come from whoever signed the running app. An unsigned
/// app (a development build) has no publisher to compare with; the
/// installer's signature still had to be valid to get here.
fn signer_accepted(own: Option<&str>, installer: &str) -> bool {
    own.is_none() || own == Some(installer)
}

/// Launches the verified NSIS installer via ShellExecuteW with "runas"
/// so Windows shows the UAC elevation prompt, and /autoupdate for the
/// progress-only install. The checksum is checked once more first.
#[allow(unsafe_code)]
pub fn launch_installer(installer: &VerifiedInstaller) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    installer.check_hash()?;

    let path_wide: Vec<u16> = installer
        .path()
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let params: Vec<u16> = "/autoupdate\0".encode_utf16().collect();

    let result = unsafe {
        winapi::um::shellapi::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            path_wide.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            winapi::um::winuser::SW_SHOWNORMAL,
        )
    };

    if result as usize > 32 {
        Ok(())
    } else {
        Err(format!(
            "Failed to launch installer (ShellExecuteW returned {})",
            result as usize
        ))
    }
}

fn is_newer(remote: &str, current: &str) -> bool {
    parse_semver(remote) > parse_semver(current)
}

fn parse_semver(v: &str) -> (u64, u64, u64) {
    let v = v.trim_start_matches('v');
    let mut parts = v.splitn(3, '.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts
        .next()
        .and_then(|s| s.split('-').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (major, minor, patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_patch_detected() {
        assert!(is_newer("1.25.1", "1.25.0"));
    }

    #[test]
    fn same_version_not_newer() {
        assert!(!is_newer("1.25.0", "1.25.0"));
    }

    #[test]
    fn older_not_newer() {
        assert!(!is_newer("1.24.9", "1.25.0"));
    }

    #[test]
    fn major_bump_detected() {
        assert!(is_newer("2.0.0", "1.25.0"));
    }

    #[test]
    fn v_prefix_stripped() {
        assert!(is_newer("v1.26.0", "1.25.0"));
    }

    fn manifest(platform: serde_json::Value) -> LatestManifest {
        LatestManifest {
            version: "9.9.9".to_string(),
            notes: None,
            platforms: serde_json::json!({ "windows-x86_64": platform }),
        }
    }

    const GOOD_URL: &str =
        "https://github.com/dvalfrid/rigstats/releases/download/v9.9.9/RIGStats_9.9.9_x64-setup.exe";
    const GOOD_HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn manifest_with_release_url_and_checksum_is_accepted() {
        let info = update_info(
            manifest(serde_json::json!({ "url": GOOD_URL, "sha256": GOOD_HASH.to_uppercase() })),
            false,
        )
        .unwrap();
        assert_eq!(info.url, GOOD_URL);
        assert_eq!(info.sha256, GOOD_HASH);
    }

    #[test]
    fn manifest_without_checksum_is_refused() {
        assert!(update_info(manifest(serde_json::json!({ "url": GOOD_URL })), false).is_err());
        let short = manifest(serde_json::json!({ "url": GOOD_URL, "sha256": "abc" }));
        assert!(update_info(short, false).is_err());
    }

    #[test]
    fn manifest_pointing_elsewhere_is_refused() {
        for url in [
            "https://example.com/RIGStats_9.9.9_x64-setup.exe",
            "http://github.com/dvalfrid/rigstats/releases/download/v9.9.9/setup.exe",
            "https://github.com/dvalfrid/rigstats/releases/download/../../../other/repo/setup.exe",
            "https://github.com/dvalfrid/rigstats/releases/download/v9.9.9/setup.exe?x=1",
            "https://github.com/dvalfrid/rigstats/releases/download/",
        ] {
            let m = manifest(serde_json::json!({ "url": url, "sha256": GOOD_HASH }));
            assert!(update_info(m, false).is_err(), "{url}");
        }
    }

    #[test]
    fn custom_manifest_may_point_anywhere() {
        let m = manifest(
            serde_json::json!({ "url": "http://localhost:8000/setup.exe", "sha256": GOOD_HASH }),
        );
        assert!(update_info(m, true).is_ok());
    }

    #[test]
    fn installer_must_share_the_apps_signer() {
        assert!(signer_accepted(Some("CN=Code By AB"), "CN=Code By AB"));
        assert!(!signer_accepted(Some("CN=Code By AB"), "CN=Someone Else"));
        assert!(signer_accepted(None, "CN=Anyone"));
    }

    /// A download folder of its own — `fresh_download_dir` clears the shared
    /// one, which parallel tests must not do to each other.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rigstats-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_changed_download_fails_the_checksum() {
        let dir = test_dir("checksum");
        let path = dir.join("rigstats-setup.exe");
        std::fs::write(&path, b"installer").unwrap();
        let digest = ring::digest::digest(&ring::digest::SHA256, b"installer");
        let installer = VerifiedInstaller {
            path: path.clone(),
            sha256: hex(digest.as_ref()),
            lock: std::fs::File::open(&path).unwrap(),
        };
        assert!(installer.check_hash().is_ok());

        std::fs::write(&path, b"something else").unwrap();
        assert!(installer.check_hash().is_err());
        drop(installer);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_locked_download_cannot_be_replaced() {
        let dir = test_dir("lock");
        let path = dir.join("rigstats-setup.exe");
        std::fs::write(&path, b"installer").unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();

        assert!(std::fs::write(&path, b"swapped").is_err());
        assert!(std::fs::remove_file(&path).is_err());
        assert!(std::fs::rename(&path, dir.join("moved.exe")).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"installer");
        drop(lock);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
