use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config;
use crate::errors::CliError;
use crate::output::OutputFormat;

const REPO: &str = "hypurrclaw/hyperliquid-cli";
const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/hypurrclaw/hyperliquid-cli/releases/latest";
const INSTALL_COMMAND: &str = "curl -fsSLO https://raw.githubusercontent.com/hypurrclaw/hyperliquid-cli/main/install.sh && sh install.sh";
const VERSION_CACHE_FILE: &str = "version.json";
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(20 * 60 * 60);
const ENV_NO_UPDATE_CHECK: &str = "HYPERLIQUID_NO_UPDATE_CHECK";
const ENV_AGENT: &str = "HYPERLIQUID_AGENT";

#[derive(Debug, Clone, Deserialize, Serialize)]
struct VersionCache {
    latest_version: String,
    last_checked_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct ReleaseInfo {
    tag_name: String,
}

#[derive(Debug, Clone, Serialize)]
struct UpdateResult {
    status: &'static str,
    from: String,
    to: String,
    path: Option<PathBuf>,
}

/// Start a passive update check. Returns a `JoinHandle` when a background
/// cache refresh was spawned, or `None` when checks are disabled or no
/// refresh is needed. The caller should await the handle with a short
/// timeout before exit so the cache is written even for fast commands.
pub fn maybe_start_update_check(
    format: OutputFormat,
    disabled: bool,
) -> Option<tokio::task::JoinHandle<()>> {
    if disabled
        || format == OutputFormat::Json
        || env_truthy(ENV_NO_UPDATE_CHECK)
        || env_truthy(ENV_AGENT)
        || !std::io::stderr().is_terminal()
    {
        return None;
    }

    let cache_path = version_cache_path()?;

    let current_version = env!("CARGO_PKG_VERSION");
    let cached = read_version_cache(&cache_path);
    if let Some(cache) = cached.as_ref()
        && is_newer_version(&cache.latest_version, current_version)
    {
        eprintln!(
            "A new version of hyperliquid is available: {} -> {}",
            current_version,
            normalize_version(&cache.latest_version)
        );
        eprintln!("Update: {INSTALL_COMMAND}");
    }

    if cached
        .as_ref()
        .is_none_or(|cache| cache_age(cache) >= UPDATE_CHECK_INTERVAL)
    {
        return Some(tokio::spawn(async move {
            let _ = refresh_version_cache(cache_path).await;
        }));
    }

    None
}

/// Wait for a pending background update-check handle with a bounded timeout
/// so the cache gets written even for fast commands. Silently ignores errors
/// (cancelled, timed out, or panicked) — the cache is best-effort.
pub async fn flush_update_check(handle: Option<tokio::task::JoinHandle<()>>) {
    if let Some(h) = handle {
        let _ = tokio::time::timeout(Duration::from_secs(2), h).await;
    }
}

pub async fn update(format: OutputFormat, dry_run: bool) -> Result<(), anyhow::Error> {
    let current_version = env!("CARGO_PKG_VERSION");
    let latest_version = fetch_latest_release_version().await?;
    let normalized_latest = normalize_version(&latest_version);
    let current_exe = std::env::current_exe().map_err(|err| {
        CliError::Internal(anyhow::anyhow!(
            "failed to resolve current executable: {err}"
        ))
    })?;

    if !is_newer_version(&latest_version, current_version) {
        print_update_result(
            format,
            &UpdateResult {
                status: "up_to_date",
                from: current_version.to_string(),
                to: normalized_latest,
                path: Some(current_exe),
            },
        )?;
        return Ok(());
    }

    if dry_run {
        print_update_result(
            format,
            &UpdateResult {
                status: "would_update",
                from: current_version.to_string(),
                to: normalized_latest,
                path: Some(current_exe),
            },
        )?;
        return Ok(());
    }

    let target = current_target()?;
    let asset = format!("hyperliquid-{target}.tar.gz");
    let base_url = format!("https://github.com/{REPO}/releases/download/{latest_version}/{asset}");
    let archive = download_bytes(&base_url).await?;
    let checksum = download_text(&format!("{base_url}.sha256")).await?;
    verify_sha256(&archive, &asset, &checksum)?;

    let tmpdir = create_temp_dir()?;
    let archive_path = tmpdir.path().join(&asset);
    std::fs::write(&archive_path, archive)?;
    extract_archive(&archive_path, tmpdir.path()).await?;
    let new_binary = find_extracted_binary(tmpdir.path())?;
    replace_current_binary(&current_exe, &new_binary)?;

    print_update_result(
        format,
        &UpdateResult {
            status: "updated",
            from: current_version.to_string(),
            to: normalized_latest,
            path: Some(current_exe),
        },
    )?;
    Ok(())
}

async fn refresh_version_cache(cache_path: PathBuf) -> Result<(), anyhow::Error> {
    let latest_version = fetch_latest_release_version().await?;
    let cache = VersionCache {
        latest_version,
        last_checked_at: now_unix_seconds(),
    };
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(cache_path, format!("{}\n", serde_json::to_string(&cache)?))?;
    Ok(())
}

async fn fetch_latest_release_version() -> Result<String, anyhow::Error> {
    let release = reqwest::Client::new()
        .get(LATEST_RELEASE_URL)
        .header(reqwest::header::USER_AGENT, "hyperliquid-cli")
        .send()
        .await
        .map_err(|err| CliError::Unavailable(format!("update check failed: {err}")))?
        .error_for_status()
        .map_err(|err| CliError::Unavailable(format!("update check failed: {err}")))?
        .json::<ReleaseInfo>()
        .await
        .map_err(|err| CliError::Unavailable(format!("invalid update response: {err}")))?;
    Ok(release.tag_name)
}

async fn download_bytes(url: &str) -> Result<Vec<u8>, anyhow::Error> {
    reqwest::Client::new()
        .get(url)
        .header(reqwest::header::USER_AGENT, "hyperliquid-cli")
        .send()
        .await
        .map_err(|err| CliError::Unavailable(format!("download failed: {err}")))?
        .error_for_status()
        .map_err(|err| CliError::Unavailable(format!("download failed: {err}")))?
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|err| CliError::Unavailable(format!("download failed: {err}")).into())
}

async fn download_text(url: &str) -> Result<String, anyhow::Error> {
    reqwest::Client::new()
        .get(url)
        .header(reqwest::header::USER_AGENT, "hyperliquid-cli")
        .send()
        .await
        .map_err(|err| CliError::Unavailable(format!("download failed: {err}")))?
        .error_for_status()
        .map_err(|err| CliError::Unavailable(format!("download failed: {err}")))?
        .text()
        .await
        .map_err(|err| CliError::Unavailable(format!("download failed: {err}")).into())
}

fn version_cache_path() -> Option<PathBuf> {
    config::config_dir().map(|dir| dir.join(VERSION_CACHE_FILE))
}

fn read_version_cache(path: &Path) -> Option<VersionCache> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn cache_age(cache: &VersionCache) -> Duration {
    let elapsed = now_unix_seconds().saturating_sub(cache.last_checked_at);
    if elapsed < 0 {
        return Duration::MAX;
    }
    Duration::from_secs(elapsed as u64)
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn is_newer_version(candidate: &str, current: &str) -> bool {
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

fn normalize_version(version: &str) -> String {
    version.trim().trim_start_matches('v').to_string()
}

fn current_target() -> Result<&'static str, CliError> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        (os, arch) => Err(CliError::Unsupported(format!(
            "update is not supported on {os}/{arch}"
        ))),
    }
}

fn verify_sha256(bytes: &[u8], asset: &str, checksum_text: &str) -> Result<(), CliError> {
    use sha2::{Digest, Sha256};

    let expected = checksum_text
        .lines()
        .find(|line| line.contains(asset))
        .and_then(|line| line.split_whitespace().next())
        .ok_or_else(|| {
            CliError::Unavailable(format!("release checksum did not include {asset}"))
        })?;

    let actual = hex::encode(Sha256::digest(bytes));
    if actual != expected {
        return Err(CliError::Unavailable(format!(
            "checksum mismatch for {asset}: expected {expected}, got {actual}"
        )));
    }
    Ok(())
}

async fn extract_archive(archive_path: &Path, destination: &Path) -> Result<(), anyhow::Error> {
    let status = tokio::process::Command::new("tar")
        .arg("-xzf")
        .arg(archive_path)
        .arg("-C")
        .arg(destination)
        .status()
        .await
        .map_err(|err| CliError::Unavailable(format!("failed to run tar: {err}")))?;
    if !status.success() {
        return Err(CliError::Unavailable(format!("tar exited with status {status}")).into());
    }
    Ok(())
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn create_temp_dir() -> Result<TempDir, anyhow::Error> {
    for _ in 0..16 {
        let path = std::env::temp_dir().join(format!(
            "hyperliquid-update-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        match create_secure_temp_dir(&path) {
            Ok(()) => return Ok(TempDir { path }),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        }
    }
    Err(CliError::Internal(anyhow::anyhow!(
        "failed to create a unique temporary update directory"
    ))
    .into())
}

fn create_secure_temp_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(path)
    }
}

fn find_extracted_binary(tmpdir: &Path) -> Result<PathBuf, CliError> {
    let direct = tmpdir.join("hyperliquid");
    if direct.is_file() {
        return Ok(direct);
    }
    find_extracted_binary_recursive(tmpdir)?.ok_or_else(|| {
        CliError::Unavailable(
            "release archive did not contain executable 'hyperliquid'".to_string(),
        )
    })
}

fn find_extracted_binary_recursive(dir: &Path) -> Result<Option<PathBuf>, CliError> {
    for entry in std::fs::read_dir(dir).map_err(|err| {
        CliError::Internal(anyhow::anyhow!("failed to inspect release archive: {err}"))
    })? {
        let path = entry
            .map_err(|err| {
                CliError::Internal(anyhow::anyhow!("failed to inspect release archive: {err}"))
            })?
            .path();
        if path.file_name().is_some_and(|name| name == "hyperliquid") && path.is_file() {
            return Ok(Some(path));
        }
        if path.is_dir()
            && let Some(binary) = find_extracted_binary_recursive(&path)?
        {
            return Ok(Some(binary));
        }
    }
    Ok(None)
}

fn replace_current_binary(current_exe: &Path, new_binary: &Path) -> Result<(), anyhow::Error> {
    let parent = current_exe.parent().ok_or_else(|| {
        CliError::Internal(anyhow::anyhow!(
            "current executable has no parent directory"
        ))
    })?;
    let tmp_path = parent.join(format!(".hyperliquid-update-{}", std::process::id()));
    std::fs::copy(new_binary, &tmp_path).map_err(|err| {
        CliError::Unavailable(format!(
            "failed to write updated binary next to {}: {err}",
            current_exe.display()
        ))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755)).map_err(
            |err| {
                let _ = std::fs::remove_file(&tmp_path);
                CliError::Unavailable(format!(
                    "failed to set permissions on {}: {err}",
                    tmp_path.display()
                ))
            },
        )?;
    }
    std::fs::rename(&tmp_path, current_exe).map_err(|err| {
        let _ = std::fs::remove_file(&tmp_path);
        CliError::Unavailable(format!(
            "failed to replace {}: {err}",
            current_exe.display()
        ))
    })?;
    Ok(())
}

fn print_update_result(format: OutputFormat, result: &UpdateResult) -> Result<(), anyhow::Error> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string(result)?),
        OutputFormat::Table => {
            println!("status\tfrom\tto\tpath");
            println!(
                "{}\t{}\t{}\t{}",
                result.status,
                result.from,
                result.to,
                result
                    .path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            );
        }
        OutputFormat::Pretty => match result.status {
            "updated" => println!("Updated hyperliquid {} -> {}", result.from, result.to),
            "would_update" => println!("Would update hyperliquid {} -> {}", result.from, result.to),
            "up_to_date" => println!("hyperliquid is up to date ({})", result.from),
            status => println!("hyperliquid update status: {status}"),
        },
    }
    Ok(())
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
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
    fn checksum_requires_target_asset_line() {
        let checksum = format!("{}  other-asset.tar.gz", "0".repeat(64));
        let err = verify_sha256(b"archive", "hyperliquid-target.tar.gz", &checksum).unwrap_err();
        assert!(
            err.to_string()
                .contains("release checksum did not include hyperliquid-target.tar.gz")
        );
    }

    #[test]
    fn extracted_binary_searches_nested_directories() {
        let tmpdir = create_temp_dir().unwrap();
        let nested = tmpdir.path().join("release").join("bin");
        std::fs::create_dir_all(&nested).unwrap();
        let binary = nested.join("hyperliquid");
        std::fs::write(&binary, b"binary").unwrap();

        assert_eq!(find_extracted_binary(tmpdir.path()).unwrap(), binary);
    }

    #[test]
    fn temp_dir_uses_randomized_update_prefix() {
        let tmpdir = create_temp_dir().unwrap();
        let name = tmpdir.path().file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("hyperliquid-update-"));
        assert_ne!(
            tmpdir.path(),
            &std::env::temp_dir().join(format!(
                "hyperliquid-update-{}-{}",
                std::process::id(),
                now_unix_seconds()
            ))
        );
    }

    #[cfg(unix)]
    #[test]
    fn temp_dir_is_owner_only_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let tmpdir = create_temp_dir().unwrap();
        let mode = std::fs::metadata(tmpdir.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn current_target_supports_this_platform() {
        let _ = current_target();
    }
}
