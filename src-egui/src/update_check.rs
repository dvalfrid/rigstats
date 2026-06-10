use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};

const LATEST_JSON_URL: &str =
    "https://github.com/dvalfrid/rigstats/releases/latest/download/latest.json";

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
}

/// Result of a version check.
pub enum CheckResult {
    /// Already on the latest version.
    UpToDate,
    /// A newer version is available.
    UpdateAvailable(UpdateInfo),
}

/// Fetch `latest.json` and compare against the current build version.
pub fn check() -> Result<CheckResult, String> {
    let resp = ureq::get(LATEST_JSON_URL)
        .call()
        .map_err(|e| format!("HTTP error: {e}"))?;

    let manifest: LatestManifest = resp
        .into_json()
        .map_err(|e| format!("JSON parse error: {e}"))?;

    let remote = &manifest.version;
    let current = env!("CARGO_PKG_VERSION");
    let notes = manifest.notes.unwrap_or_default();

    if !is_newer(remote, current) {
        return Ok(CheckResult::UpToDate);
    }

    let url = manifest
        .platforms
        .get("windows-x86_64")
        .and_then(|p: &serde_json::Value| p.get("url"))
        .and_then(|u: &serde_json::Value| u.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "No windows-x86_64 URL in manifest".to_string())?;

    Ok(CheckResult::UpdateAvailable(UpdateInfo {
        version: remote.clone(),
        notes,
        url,
    }))
}

/// Downloads `url` to `dest`, calling `on_progress(downloaded, total_or_0)` periodically.
pub fn download(url: &str, dest: &Path, on_progress: impl Fn(u64, u64)) -> Result<(), String> {
    let resp = ureq::get(url)
        .call()
        .map_err(|e| format!("Download HTTP error: {e}"))?;

    let total: u64 = resp
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| format!("Cannot create file: {e}"))?;

    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Read error: {e}"))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| format!("Write error: {e}"))?;
        downloaded += n as u64;
        on_progress(downloaded, total);
    }

    Ok(())
}

/// Returns the path where the installer should be saved in the temp dir.
pub fn installer_temp_path(version: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rigstats-update-v{version}.exe"))
}

/// Launches the downloaded NSIS installer (triggers Windows UAC).
pub fn launch_installer(path: &Path) -> Result<(), String> {
    std::process::Command::new(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to launch installer: {e}"))
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
}
