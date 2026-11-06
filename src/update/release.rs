//! What the notifier and the self-updater both need to know about releases:
//! where they live, how versions compare, which archive fits this platform, and
//! how the running binary was installed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::errors::CliError;
use crate::http_api;
use crate::settings::env;

/// Must match the repository and archive names the release workflow publishes.
pub(super) const REPO: &str = "hypurrclaw/hyperliquid-cli";
const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/hypurrclaw/hyperliquid-cli/releases/latest";
const USER_AGENT: &str = "hyperliquid-cli";
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Deserialize)]
struct ReleaseInfo {
    tag_name: String,
}

/// One HTTP client for every release request of a run.
pub(super) struct ReleaseClient {
    client: reqwest::Client,
}

impl ReleaseClient {
    pub fn new() -> Result<Self, CliError> {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .map(|client| Self { client })
            .map_err(|err| CliError::Unavailable(format!("update check failed: {err}")))
    }

    async fn get(
        &self,
        url: &str,
        failure: &str,
        timeout: Option<Duration>,
    ) -> Result<reqwest::Response, CliError> {
        let mut request = self.client.get(url);
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        let response = request
            .send()
            .await
            .map_err(|err| CliError::Unavailable(format!("{failure}: {err}")))?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let body = response.text().await.unwrap_or_default();
        Err(http_api::failed_reply(status, &body))
    }

    /// The tag of the latest published release.
    pub async fn latest_version(&self) -> Result<String, CliError> {
        let release = self
            .get(
                LATEST_RELEASE_URL,
                "update check failed",
                Some(FETCH_TIMEOUT),
            )
            .await?
            .json::<ReleaseInfo>()
            .await
            .map_err(|err| CliError::Unavailable(format!("invalid update response: {err}")))?;
        Ok(release.tag_name)
    }

    pub async fn download_bytes(&self, url: &str) -> Result<Vec<u8>, CliError> {
        self.get(url, "download failed", None)
            .await?
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|err| CliError::Unavailable(format!("download failed: {err}")))
    }

    pub async fn download_text(&self, url: &str) -> Result<String, CliError> {
        self.get(url, "download failed", None)
            .await?
            .text()
            .await
            .map_err(|err| CliError::Unavailable(format!("download failed: {err}")))
    }
}

pub(super) fn is_newer_version(candidate: &str, current: &str) -> bool {
    let candidate = ParsedVersion::parse(candidate);
    let current = ParsedVersion::parse(current);
    for index in 0..candidate.parts.len().max(current.parts.len()) {
        let left = *candidate.parts.get(index).unwrap_or(&0);
        let right = *current.parts.get(index).unwrap_or(&0);
        if left > right {
            return true;
        }
        if left < right {
            return false;
        }
    }
    !candidate.prerelease && current.prerelease
}

#[derive(Debug)]
struct ParsedVersion {
    parts: Vec<u64>,
    prerelease: bool,
}

impl ParsedVersion {
    fn parse(version: &str) -> Self {
        let normalized = normalize_version(version);
        let (core, prerelease) = match normalized.split_once('-') {
            Some((core, suffix)) => (core, !suffix.is_empty()),
            None => (normalized.as_str(), false),
        };
        Self {
            parts: core
                .split(|ch: char| !ch.is_ascii_digit())
                .filter(|part| !part.is_empty())
                .filter_map(|part| part.parse::<u64>().ok())
                .collect(),
            prerelease,
        }
    }
}

pub(super) fn normalize_version(version: &str) -> String {
    version.trim().trim_start_matches('v').to_string()
}

pub(super) fn current_release_asset() -> Result<&'static str, CliError> {
    release_asset_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn release_asset_for(os: &str, arch: &str) -> Result<&'static str, CliError> {
    match (os, arch) {
        ("macos", "x86_64") => Ok("hyperliquid-macos-x86_64.tar.gz"),
        ("macos", "aarch64") => Ok("hyperliquid-macos-aarch64.tar.gz"),
        ("linux", "x86_64") => Ok("hyperliquid-linux-x86_64.tar.gz"),
        ("linux", "aarch64") => Ok("hyperliquid-linux-aarch64.tar.gz"),
        (os, arch) => Err(CliError::Unsupported(format!(
            "update is not supported on {os}/{arch}"
        ))),
    }
}

/// How the running binary was installed. Package-managed and cargo installs
/// must be upgraded with their own tool, not overwritten in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InstallMethod {
    Homebrew,
    Cargo,
    Standalone,
}

impl InstallMethod {
    pub fn detect(exe: &Path) -> Self {
        let home = dirs::home_dir();
        let cargo_home = std::env::var_os(env::CARGO_HOME).map(PathBuf::from);
        let resolved = std::fs::canonicalize(exe).ok();
        [Some(exe), resolved.as_deref()]
            .into_iter()
            .flatten()
            .map(|path| Self::classify(path, home.as_deref(), cargo_home.as_deref()))
            .find(|method| *method != Self::Standalone)
            .unwrap_or(Self::Standalone)
    }

    fn classify(exe: &Path, home: Option<&Path>, cargo_home: Option<&Path>) -> Self {
        if exe
            .components()
            .any(|component| component.as_os_str() == "Cellar")
        {
            return Self::Homebrew;
        }
        let cargo_bins = [
            cargo_home.map(|dir| dir.join("bin")),
            home.map(|dir| dir.join(".cargo").join("bin")),
        ];
        if cargo_bins.iter().flatten().any(|bin| exe.starts_with(bin)) {
            return Self::Cargo;
        }
        Self::Standalone
    }

    pub fn upgrade_command(self) -> &'static str {
        match self {
            Self::Homebrew => "brew upgrade hyperliquid",
            Self::Cargo => "cargo install hyperliquid-cli --force",
            Self::Standalone => "hyperliquid update",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Homebrew => "Homebrew",
            Self::Cargo => "cargo",
            Self::Standalone => "the install script",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare_handles_v_prefix() {
        assert!(is_newer_version("v0.2.0", "0.1.9"));
        assert!(!is_newer_version("v0.1.0", "0.1.0"));
        assert!(!is_newer_version("v0.1.0", "0.2.0"));
    }

    #[test]
    fn version_compare_handles_prereleases() {
        assert!(!is_newer_version("v1.0.0-rc.1", "1.0.0"));
        assert!(is_newer_version("v1.0.0", "1.0.0-rc.1"));
        assert!(is_newer_version("v1.0.1-rc.1", "1.0.0"));
    }

    #[test]
    fn release_asset_matches_published_unix_archive_names() {
        assert_eq!(
            release_asset_for("linux", "x86_64").unwrap(),
            "hyperliquid-linux-x86_64.tar.gz"
        );
        assert_eq!(
            release_asset_for("linux", "aarch64").unwrap(),
            "hyperliquid-linux-aarch64.tar.gz"
        );
        assert_eq!(
            release_asset_for("macos", "x86_64").unwrap(),
            "hyperliquid-macos-x86_64.tar.gz"
        );
        assert_eq!(
            release_asset_for("macos", "aarch64").unwrap(),
            "hyperliquid-macos-aarch64.tar.gz"
        );
    }

    #[test]
    fn windows_self_update_is_explicitly_unsupported() {
        let err = release_asset_for("windows", "x86_64").unwrap_err();
        assert!(
            err.to_string()
                .contains("update is not supported on windows/x86_64")
        );
    }

    #[test]
    fn current_release_asset_supports_this_platform() {
        let _ = current_release_asset();
    }

    #[test]
    fn install_method_detects_homebrew_and_cargo() {
        let home = Path::new("/home/me");
        let classify = |exe: &str, cargo_home: Option<&Path>| {
            InstallMethod::classify(Path::new(exe), Some(home), cargo_home)
        };
        assert_eq!(
            classify(
                "/opt/homebrew/Cellar/hyperliquid/0.11.1/bin/hyperliquid",
                None
            ),
            InstallMethod::Homebrew
        );
        assert_eq!(
            classify(
                "/home/linuxbrew/.linuxbrew/Cellar/hyperliquid/1/bin/hyperliquid",
                None
            ),
            InstallMethod::Homebrew
        );
        assert_eq!(
            classify("/home/me/.cargo/bin/hyperliquid", None),
            InstallMethod::Cargo
        );
        assert_eq!(
            classify("/opt/cargo/bin/hyperliquid", Some(Path::new("/opt/cargo"))),
            InstallMethod::Cargo
        );
        assert_eq!(
            classify("/usr/local/bin/hyperliquid", None),
            InstallMethod::Standalone
        );
        assert_eq!(
            InstallMethod::Homebrew.upgrade_command(),
            "brew upgrade hyperliquid"
        );
        assert_eq!(
            InstallMethod::Standalone.upgrade_command(),
            "hyperliquid update"
        );
    }
}
