use rigstats_backend::settings::UpdateChannel;
use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// This build's version as released: the release workflow passes the tag's
/// (`RIGSTATS_VERSION`, e.g. `1.51.0-rc.2`), so a release candidate knows it
/// is one; elsewhere `Cargo.toml`'s. Shown in About and Status and compared
/// by the updater (#535). The Control Center's handshake with the service
/// keeps `CARGO_PKG_VERSION`.
pub const APP_VERSION: &str = match option_env!("RIGSTATS_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// The newest releases, release candidates included — `releases/latest`
/// skips pre-releases.
const RELEASES_API_URL: &str =
    "https://api.github.com/repos/dvalfrid/rigstats/releases?per_page=20";

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

/// The release's highlights, written for users before the release
/// (`release-highlights.md`, see "Release routine" in docs/release.md).
pub const BUNDLED_HIGHLIGHTS: &str = include_str!("../../release-highlights.md");

/// The highlights file's version and its item lines (`- …`), or `None`
/// when it has neither. HTML comments (the file's instructions and example)
/// are skipped.
pub fn parse_highlights(text: &str) -> Option<(String, Vec<String>)> {
    let mut rest = text;
    let mut visible = String::new();
    while let Some(start) = rest.find("<!--") {
        visible.push_str(&rest[..start]);
        rest = rest[start..]
            .find("-->")
            .map_or("", |end| &rest[start + end + 3..]);
    }
    visible.push_str(rest);
    let version = visible
        .lines()
        .find_map(|l| l.trim().strip_prefix("# "))?
        .trim()
        .trim_start_matches('v')
        .to_owned();
    let items: Vec<String> = visible
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("- "))
        .map(str::to_owned)
        .collect();
    (!version.is_empty() && !items.is_empty()).then_some((version, items))
}

/// `changelog` with a `### Highlights` section first under the heading of
/// the version `highlights` is for. Unchanged when that version isn't in
/// it, so an old highlights file never shows up under a new release. The
/// Release workflow does the same for `latest.json`'s notes.
pub fn with_highlights(changelog: &str, highlights: &str) -> String {
    let Some((version, items)) = parse_highlights(highlights) else {
        return changelog.to_owned();
    };
    let is_heading = |line: &str| {
        line.strip_prefix("## ").is_some_and(|h| {
            let h = h.trim_start_matches('[').trim_start_matches('v');
            h.strip_prefix(version.as_str()).is_some_and(|after| {
                after.is_empty()
                    || !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '.')
            })
        })
    };
    let mut out = String::with_capacity(changelog.len() + 512);
    let mut done = false;
    for line in changelog.lines() {
        out.push_str(line);
        out.push('\n');
        if !done && is_heading(line.trim()) {
            out.push_str("\n### Highlights\n\n");
            for item in &items {
                out.push_str(item);
                out.push('\n');
            }
            done = true;
        }
    }
    out
}

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

/// 15 s to connect (TLS handshake included), 30 s for each read.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .build()
        .new_agent()
}

/// `read_json` stops at 10 MB — far above `latest.json` or a page of
/// releases.
fn fetch_manifest(url: &str) -> Result<LatestManifest, String> {
    agent()
        .get(url)
        .call()
        .map_err(|e| format!("HTTP error: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("JSON parse error: {e}"))
}

/// Fetch the channel's newest `latest.json` and compare it against this
/// build's version. On `ReleaseCandidates` that is the newest release
/// candidate's when it is newer than the newest release's; if the release
/// candidates can't be listed (no network, GitHub's API limit), the
/// release's.
pub fn check(channel: UpdateChannel) -> Result<CheckResult, String> {
    let mut manifest = fetch_manifest(LATEST_JSON_URL)?;
    if channel == UpdateChannel::ReleaseCandidates && CUSTOM_MANIFEST_URL.is_none() {
        if let Some(rc) = newest_candidate_manifest()
            .ok()
            .flatten()
            .and_then(|url| fetch_manifest(&url).ok())
        {
            if is_newer(&rc.version, &manifest.version) {
                manifest = rc;
            }
        }
    }

    if !is_newer(&manifest.version, APP_VERSION) {
        return Ok(CheckResult::UpToDate);
    }
    update_info(manifest, CUSTOM_MANIFEST_URL.is_some()).map(CheckResult::UpdateAvailable)
}

#[derive(Debug, Deserialize)]
struct ApiRelease {
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Debug, Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

/// The `latest.json` of the newest release candidate, from GitHub's list of
/// releases (newest first).
fn newest_candidate_manifest() -> Result<Option<String>, String> {
    let releases: Vec<ApiRelease> = agent()
        .get(RELEASES_API_URL)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "RIGStats")
        .call()
        .map_err(|e| format!("HTTP error: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("JSON parse error: {e}"))?;
    Ok(candidate_manifest_url(&releases).map(str::to_owned))
}

/// The first published pre-release with a `latest.json` from this
/// project's releases. A release being built is a pre-release without one
/// yet, and is passed over.
fn candidate_manifest_url(releases: &[ApiRelease]) -> Option<&str> {
    releases
        .iter()
        .filter(|r| r.prerelease && !r.draft)
        .find_map(|r| {
            r.assets
                .iter()
                .find(|a| a.name == "latest.json" && is_release_url(&a.browser_download_url))
        })
        .map(|a| a.browser_download_url.as_str())
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

    let total: u64 = resp.body().content_length().unwrap_or(0);

    // Unlimited in ureq; the loop below stops at `MAX_INSTALLER_BYTES`.
    let mut reader = resp.into_body().into_reader();
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
    // ShellExecuteW's result is an HINSTANCE only for history's sake: above
    // 32 means it started, 32 and below is the error code.
    // SAFETY: `path_wide` is NUL-terminated and outlives the call, the other
    // strings are static; no window.
    let result = unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(
            None,
            windows::core::w!("runas"),
            windows::core::PCWSTR(path_wide.as_ptr()),
            windows::core::w!("/autoupdate"),
            None,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    }
    .0 as usize;

    if result > 32 {
        Ok(())
    } else {
        Err(format!(
            "Failed to launch installer (ShellExecuteW returned {})",
            result
        ))
    }
}

fn is_newer(remote: &str, current: &str) -> bool {
    parse_semver(remote) > parse_semver(current)
}

/// One dot-separated part of a pre-release label: numbers sort as numbers
/// and before words (semver), so `rc.10` comes after `rc.9`.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PreRelease {
    Number(u64),
    Word(String),
}

/// Sort key: major, minor, patch, then a release after all of its
/// pre-releases (`1.51.0-rc.2` < `1.51.0`), then the pre-release label.
fn parse_semver(v: &str) -> (u64, u64, u64, bool, Vec<PreRelease>) {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = v.split_once('-').unwrap_or((v, ""));
    let mut parts = core.splitn(3, '.');
    let mut next = || parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let (major, minor, patch) = (next(), next(), next());
    let label = pre
        .split('.')
        .filter(|p| !p.is_empty())
        .map(|p| {
            p.parse()
                .map_or_else(|_| PreRelease::Word(p.to_owned()), PreRelease::Number)
        })
        .collect::<Vec<_>>();
    (major, minor, patch, label.is_empty(), label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_candidates_sort_between_releases() {
        assert!(is_newer("1.51.0-rc.1", "1.50.0"));
        assert!(is_newer("1.51.0-rc.2", "1.51.0-rc.1"));
        assert!(is_newer("1.51.0-rc.10", "1.51.0-rc.9"));
        assert!(is_newer("1.51.0", "1.51.0-rc.2"));
        assert!(!is_newer("1.51.0-rc.2", "1.51.0"));
        assert!(!is_newer("1.51.0-rc.2", "1.51.0-rc.2"));
        assert!(!is_newer("1.50.0", "1.51.0-rc.1"));
    }

    fn api_release(prerelease: bool, draft: bool, assets: &[(&str, &str)]) -> ApiRelease {
        ApiRelease {
            draft,
            prerelease,
            assets: assets
                .iter()
                .map(|(name, url)| ApiAsset {
                    name: (*name).into(),
                    browser_download_url: (*url).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_newest_published_candidate_with_a_manifest_is_taken() {
        const RC2: &str =
            "https://github.com/dvalfrid/rigstats/releases/download/v1.51.0-rc.2/latest.json";
        const RC1: &str =
            "https://github.com/dvalfrid/rigstats/releases/download/v1.51.0-rc.1/latest.json";
        const STABLE: &str =
            "https://github.com/dvalfrid/rigstats/releases/download/v1.50.0/latest.json";
        let releases = [
            // A release still being built: held back as a pre-release, no manifest yet.
            api_release(true, false, &[("RIGStats_1.52.0_x64-setup.exe", "x")]),
            api_release(true, true, &[("latest.json", RC2)]),
            api_release(false, false, &[("latest.json", STABLE)]),
            api_release(true, false, &[("latest.json", RC2)]),
            api_release(true, false, &[("latest.json", RC1)]),
        ];
        assert_eq!(candidate_manifest_url(&releases), Some(RC2));
    }

    #[test]
    fn a_candidate_manifest_outside_this_project_is_ignored() {
        let releases = [api_release(
            true,
            false,
            &[("latest.json", "https://example.com/latest.json")],
        )];
        assert_eq!(candidate_manifest_url(&releases), None);
        assert_eq!(candidate_manifest_url(&[]), None);
    }

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

    /// `lock_and_verify` on a copy of `source` with its right checksum. The
    /// test binary is unsigned, so any valid signature is accepted
    /// (`signer_accepted(None, …)`); the signature itself still has to hold.
    fn verify_copy(name: &str, source: &Path) -> Result<(), String> {
        let dir = test_dir(name);
        let path = dir.join("rigstats-setup.exe");
        std::fs::copy(source, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let sha = hex(ring::digest::digest(&ring::digest::SHA256, &bytes).as_ref());
        let result = lock_and_verify(path, &sha).map(drop);
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    #[test]
    fn a_validly_signed_download_is_verified() {
        let pawnio = Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/pawnio/PawnIO_setup.exe");
        assert_eq!(verify_copy("signed", &pawnio), Ok(()));
    }

    #[test]
    fn a_download_signed_by_an_untrusted_root_is_refused() {
        let untrusted = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/authenticode/untrusted-signed.dll");
        assert_eq!(
            verify_copy("untrusted", &untrusted),
            Err("Update refused: no valid signature (WinVerifyTrust 0x800B0109)".to_string())
        );
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

    /// Answers one HTTP request on localhost with `response` and returns the
    /// URL to ask.
    fn serve_once(response: Vec<u8>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap() == 1 {
                request.push(byte[0]);
            }
            stream.write_all(&response).unwrap();
        });
        format!("http://127.0.0.1:{port}/latest.json")
    }

    fn http_ok(body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    #[test]
    fn a_manifest_is_read_from_the_response() {
        let url = serve_once(http_ok(
            br#"{"version":"9.9.9","notes":"n","platforms":{"windows-x86_64":{}}}"#,
        ));
        let m = fetch_manifest(&url).unwrap();
        assert_eq!(m.version, "9.9.9");
        assert_eq!(m.notes.as_deref(), Some("n"));
    }

    #[test]
    fn an_error_status_is_an_http_error() {
        let url = serve_once(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        );
        let e = fetch_manifest(&url).unwrap_err();
        assert!(e.starts_with("HTTP error:") && e.contains("404"), "{e}");
    }

    #[test]
    fn a_download_reports_its_length_and_writes_every_byte() {
        let body = vec![0x5a; 200_000];
        let url = serve_once(http_ok(&body));
        let dir = test_dir("download");
        let path = dir.join("rigstats-setup.exe");
        let last = std::cell::Cell::new((0, 0));
        download_to(&url, &path, |done, total| last.set((done, total))).unwrap();
        assert_eq!(last.get(), (200_000, 200_000));
        assert_eq!(std::fs::read(&path).unwrap(), body);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Against the real GitHub: `cargo test --manifest-path src-egui/Cargo.toml
    /// real_github -- --ignored --nocapture`, with `RIGSTATS_VERSION` set to
    /// see what an older build is offered. Downloads and verifies the newest
    /// release's and release candidate's installers (checksum and
    /// signature); never runs them.
    #[test]
    #[ignore = "network: fetches from github.com"]
    fn real_github_release_and_candidates() {
        let manifest = fetch_manifest(LATEST_JSON_URL).unwrap();
        println!("latest.json: {}", manifest.version);
        let mut infos = vec![update_info(manifest, false).unwrap()];
        if let Some(url) = newest_candidate_manifest().unwrap() {
            let rc = fetch_manifest(&url).unwrap();
            println!("newest candidate: {}", rc.version);
            infos.push(update_info(rc, false).unwrap());
        }
        for channel in [UpdateChannel::Stable, UpdateChannel::ReleaseCandidates] {
            let offered = match check(channel).unwrap() {
                CheckResult::UpToDate => "nothing".to_string(),
                CheckResult::UpdateAvailable(info) => info.version,
            };
            println!("{channel:?} offers {APP_VERSION}: {offered}");
        }
        for info in &infos {
            let installer = download(info, |_, _| {}).unwrap();
            println!("{} downloaded and verified", info.version);
            drop(installer);
        }
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("rigstats-update"));
    }

    const HIGHLIGHTS: &str = "<!--
Write them under the version:
# 9.9.9
- **Not this** — the example in the comment
-->
# 1.48.0
- **Desktop per profile** — a picture of its own.
- **Wallpaper Engine** — paused while you game.
";

    const CHANGELOG: &str = "# Changelog

## [1.48.0](https://x/compare/v1.47.0...v1.48.0) (2026-11-01)


### Features

* **profiles:** something ([abc1234](https://x/commit/abc1234))

## [1.47.0](https://x) (2026-10-11)

### Features

* **profiles:** older
";

    #[test]
    fn highlights_come_from_outside_the_comment() {
        let (version, items) = parse_highlights(HIGHLIGHTS).unwrap();
        assert_eq!(version, "1.48.0");
        assert_eq!(
            items,
            [
                "- **Desktop per profile** — a picture of its own.",
                "- **Wallpaper Engine** — paused while you game."
            ]
        );
    }

    #[test]
    fn the_shipped_file_is_empty_or_a_well_formed_list() {
        // Between releases the file holds only its instructions; before a
        // release, one list for the version it publishes.
        if let Some((version, items)) = parse_highlights(BUNDLED_HIGHLIGHTS) {
            let parts: Vec<&str> = version.split('.').collect();
            assert!(
                parts.len() == 3 && parts.iter().all(|p| p.parse::<u32>().is_ok()),
                "the heading must be '# X.Y.Z', got '{version}'"
            );
            for item in &items {
                assert!(
                    item.starts_with("- **") && item.contains("** — "),
                    "each highlight reads '- **Title** — text': {item}"
                );
            }
        }
    }

    #[test]
    fn highlights_go_first_under_their_own_version_only() {
        let out = with_highlights(CHANGELOG, HIGHLIGHTS);
        let heading = out.find("## [1.48.0]").unwrap();
        let hl = out.find("### Highlights").unwrap();
        let features = out.find("### Features").unwrap();
        assert!(heading < hl && hl < features, "{out}");
        assert_eq!(out.matches("### Highlights").count(), 1);
        assert!(out.contains("- **Wallpaper Engine** — paused while you game.\n"));
    }

    #[test]
    fn highlights_for_another_version_change_nothing() {
        let old = HIGHLIGHTS.replace("# 1.48.0", "# 1.4");
        assert_eq!(with_highlights(CHANGELOG, &old), CHANGELOG);
        // 1.4 must not match 1.48.0 or 1.47.0.
        let none = HIGHLIGHTS.replace("# 1.48.0", "# 2.0.0");
        assert_eq!(with_highlights(CHANGELOG, &none), CHANGELOG);
    }

    #[test]
    fn a_plain_version_heading_matches_too() {
        let out = with_highlights(
            "## v1.48.0 - 2026-11-01\n\n### Bug Fixes\n\n- x\n",
            HIGHLIGHTS,
        );
        assert!(
            out.starts_with("## v1.48.0 - 2026-11-01\n\n### Highlights\n"),
            "{out}"
        );
    }
}
