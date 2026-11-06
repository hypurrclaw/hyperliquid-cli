use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config;
use crate::errors::CliError;
use crate::output::{OutputFormat, TableData};

const REPO: &str = "hypurrclaw/hyperliquid-cli";
const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/hypurrclaw/hyperliquid-cli/releases/latest";
const VERSION_CACHE_FILE: &str = "version.json";
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(20 * 60 * 60);
const NOTICE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// Longest a command waits for the background refresh before exiting.
const REFRESH_GRACE: Duration = Duration::from_millis(150);
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const ENV_NO_UPDATE_CHECK: &str = "HYPERLIQUID_NO_UPDATE_CHECK";
const ENV_NO_UPDATE_NOTIFIER: &str = "NO_UPDATE_NOTIFIER";

/// `last_notified_at` is optional so caches written by older versions still load.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct VersionCache {
    latest_version: String,
    last_checked_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_notified_at: Option<i64>,
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

/// Handle for one invocation's passive update check.
///
/// Created by [`maybe_start_update_check`] before the command runs and
/// consumed by [`flush_update_check`] after its result is printed.
pub struct UpdateCheck {
    cache_path: Option<PathBuf>,
    refresh: Option<tokio::task::JoinHandle<()>>,
}

impl UpdateCheck {
    fn disabled() -> Self {
        Self {
            cache_path: None,
            refresh: None,
        }
    }
}

/// Conditions under which the passive check and its notice stay silent.
#[derive(Debug, Clone, Copy, Default)]
struct Suppression {
    disabled: bool,
    json: bool,
    no_update_check_env: bool,
    agent: bool,
    ci: bool,
    no_update_notifier: bool,
    stdout_is_terminal: bool,
    stderr_is_terminal: bool,
}

impl Suppression {
    fn current(format: OutputFormat, disabled: bool) -> Self {
        Self {
            disabled,
            json: format == OutputFormat::Json,
            no_update_check_env: env_truthy(ENV_NO_UPDATE_CHECK),
            agent: crate::mode::agent_env(),
            ci: crate::mode::ci_env(),
            no_update_notifier: std::env::var_os(ENV_NO_UPDATE_NOTIFIER)
                .is_some_and(|value| !value.is_empty()),
            stdout_is_terminal: crate::output::terminal::stdout_is_terminal(),
            stderr_is_terminal: crate::output::terminal::stderr_is_terminal(),
        }
    }

    fn allows_check(self) -> bool {
        !(self.disabled
            || self.json
            || self.no_update_check_env
            || self.agent
            || self.ci
            || self.no_update_notifier
            || !self.stdout_is_terminal
            || !self.stderr_is_terminal)
    }
}

/// Start a passive update check. Returns a disabled handle in JSON, agent, CI,
/// `NO_UPDATE_NOTIFIER`, and non-terminal runs. When the cached release info
/// is stale it spawns a detached background refresh. Nothing is printed here:
/// pass the handle to [`flush_update_check`] after the command's result.
pub fn maybe_start_update_check(format: OutputFormat, disabled: bool) -> UpdateCheck {
    start_update_check(Suppression::current(format, disabled), version_cache_path())
}

fn start_update_check(suppression: Suppression, cache_path: Option<PathBuf>) -> UpdateCheck {
    let Some(cache_path) = cache_path.filter(|_| suppression.allows_check()) else {
        return UpdateCheck::disabled();
    };

    let refresh_due = read_version_cache(&cache_path)
        .is_none_or(|cache| cache_age(&cache) >= UPDATE_CHECK_INTERVAL);
    let refresh = refresh_due.then(|| tokio::spawn(refresh_version_cache(cache_path.clone())));
    UpdateCheck {
        cache_path: Some(cache_path),
        refresh,
    }
}

/// Give a pending refresh a moment to finish. The refresh is detached: if it
/// is not done in time the command still exits right away.
async fn settle_refresh(check: &mut UpdateCheck) {
    if let Some(handle) = check.refresh.take() {
        let _ = tokio::time::timeout(REFRESH_GRACE, handle).await;
    }
}

/// Finish the passive check after a successful command: wait at most
/// ~150 ms for the background refresh, then print the update notice on stderr
/// when a newer release is cached and none was shown in the last 24 hours.
pub async fn flush_update_check(mut check: UpdateCheck) {
    settle_refresh(&mut check).await;
    if let Some(cache_path) = check.cache_path.as_deref() {
        notify_if_due(cache_path);
    }
}

/// Finish the passive check after a failed command: same bounded wait, no notice.
pub async fn flush_update_check_silent(mut check: UpdateCheck) {
    settle_refresh(&mut check).await;
}

/// The update notice for a cache state, or `None` when nothing is due.
fn update_notice(
    cache: &VersionCache,
    current_version: &str,
    now: i64,
    upgrade_command: &str,
) -> Option<String> {
    if cache.latest_version.is_empty() || !is_newer_version(&cache.latest_version, current_version)
    {
        return None;
    }
    let notified_recently = cache.last_notified_at.is_some_and(|notified| {
        let elapsed = now.saturating_sub(notified);
        (0..NOTICE_INTERVAL.as_secs() as i64).contains(&elapsed)
    });
    if notified_recently {
        return None;
    }
    Some(format!(
        "hyperliquid {} is available. You have {current_version}.\nUpgrade:\n    {upgrade_command}",
        normalize_version(&cache.latest_version)
    ))
}

fn notify_if_due(cache_path: &Path) {
    let Some(cache) = read_version_cache(cache_path) else {
        return;
    };
    let upgrade_command = std::env::current_exe()
        .map(|exe| InstallMethod::detect(&exe))
        .unwrap_or(InstallMethod::Standalone)
        .upgrade_command();
    let now = now_unix_seconds();
    let Some(notice) = update_notice(&cache, env!("CARGO_PKG_VERSION"), now, upgrade_command)
    else {
        return;
    };
    eprintln!("{notice}");
    let _ = update_version_cache(cache_path, |cache| cache.last_notified_at = Some(now));
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

    let install_method = InstallMethod::detect(&current_exe);
    if install_method != InstallMethod::Standalone {
        return Err(CliError::Unsupported(format!(
            "Can't update {} in place: it was installed with {}.\nUpgrade:\n    {}",
            current_exe.display(),
            install_method.name(),
            install_method.upgrade_command()
        ))
        .into());
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

    let asset = current_release_asset()?;
    let base_url = format!("https://github.com/{REPO}/releases/download/{latest_version}/{asset}");
    let archive = download_bytes(&base_url).await?;
    let checksum = download_text(&format!("{base_url}.sha256")).await?;
    verify_sha256(&archive, asset, &checksum)?;

    let tmpdir = create_temp_dir()?;
    let archive_path = tmpdir.path().join(asset);
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

/// Read-modify-write the version cache atomically (temp file + rename).
fn update_version_cache(
    cache_path: &Path,
    change: impl FnOnce(&mut VersionCache),
) -> Result<(), anyhow::Error> {
    let mut cache = read_version_cache(cache_path).unwrap_or_default();
    change(&mut cache);
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = cache_path.with_extension("json.tmp");
    std::fs::write(&tmp_path, format!("{}\n", serde_json::to_string(&cache)?))?;
    std::fs::rename(&tmp_path, cache_path)?;
    Ok(())
}

/// Fetch the latest release and record it. A failed fetch still records
/// `last_checked_at` so the next runs back off for the check interval.
async fn refresh_version_cache(cache_path: PathBuf) {
    let latest = fetch_latest_release_version().await.ok();
    let _ = update_version_cache(&cache_path, |cache| {
        if let Some(latest) = latest {
            cache.latest_version = latest;
        }
        cache.last_checked_at = now_unix_seconds();
    });
}

async fn fetch_latest_release_version() -> Result<String, anyhow::Error> {
    let release = reqwest::Client::new()
        .get(LATEST_RELEASE_URL)
        .header(reqwest::header::USER_AGENT, "hyperliquid-cli")
        .timeout(FETCH_TIMEOUT)
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

fn current_release_asset() -> Result<&'static str, CliError> {
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

/// Copy `source` to `destination` as a new executable file. `create_new`
/// (`O_EXCL`) refuses to follow a pre-planted file or symlink at the target.
fn copy_to_new_executable(source: &Path, destination: &Path) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o755);
    }
    let mut input = std::fs::File::open(source)?;
    let mut output = options.open(destination)?;
    std::io::copy(&mut input, &mut output)?;
    output.sync_all()
}

fn replace_current_binary(current_exe: &Path, new_binary: &Path) -> Result<(), anyhow::Error> {
    let parent = current_exe.parent().ok_or_else(|| {
        CliError::Internal(anyhow::anyhow!(
            "current executable has no parent directory"
        ))
    })?;
    let tmp_path = parent.join(format!(
        ".hyperliquid-update-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    if let Err(err) = copy_to_new_executable(new_binary, &tmp_path) {
        if err.kind() != std::io::ErrorKind::AlreadyExists {
            let _ = std::fs::remove_file(&tmp_path);
        }
        return Err(CliError::Unavailable(format!(
            "failed to write updated binary next to {}: {err}",
            current_exe.display()
        ))
        .into());
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

/// How the running binary was installed. Package-managed and cargo installs
/// must be upgraded with their own tool, not overwritten in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallMethod {
    Homebrew,
    Cargo,
    Standalone,
}

impl InstallMethod {
    fn detect(exe: &Path) -> Self {
        let home = dirs::home_dir();
        let cargo_home = std::env::var_os("CARGO_HOME").map(PathBuf::from);
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

    fn upgrade_command(self) -> &'static str {
        match self {
            Self::Homebrew => "brew upgrade hyperliquid",
            Self::Cargo => "cargo install hyperliquid-cli --force",
            Self::Standalone => "hyperliquid update",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Homebrew => "Homebrew",
            Self::Cargo => "cargo",
            Self::Standalone => "the install script",
        }
    }
}

impl TableData for UpdateResult {
    fn headers(&self) -> Vec<&str> {
        vec!["Status", "From", "To", "Path"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.status.to_string(),
            self.from.clone(),
            self.to.clone(),
            self.path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

fn print_update_result(format: OutputFormat, result: &UpdateResult) -> Result<(), anyhow::Error> {
    match format {
        OutputFormat::Json | OutputFormat::Table => {
            crate::output::print_data_no_timing(result, format);
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

    fn allowing() -> Suppression {
        Suppression {
            stdout_is_terminal: true,
            stderr_is_terminal: true,
            ..Suppression::default()
        }
    }

    #[test]
    fn check_runs_only_for_interactive_human_sessions() {
        assert!(allowing().allows_check());
        for suppressed in [
            Suppression {
                disabled: true,
                ..allowing()
            },
            Suppression {
                json: true,
                ..allowing()
            },
            Suppression {
                no_update_check_env: true,
                ..allowing()
            },
            Suppression {
                agent: true,
                ..allowing()
            },
            Suppression {
                ci: true,
                ..allowing()
            },
            Suppression {
                no_update_notifier: true,
                ..allowing()
            },
            Suppression {
                stdout_is_terminal: false,
                ..allowing()
            },
            Suppression {
                stderr_is_terminal: false,
                ..allowing()
            },
        ] {
            assert!(!suppressed.allows_check(), "{suppressed:?}");
        }
    }

    #[test]
    fn suppressed_check_spawns_nothing_and_touches_no_cache() {
        let tmpdir = create_temp_dir().unwrap();
        let path = tmpdir.path().join("version.json");
        let check = start_update_check(
            Suppression {
                ci: true,
                ..allowing()
            },
            Some(path.clone()),
        );
        assert!(check.cache_path.is_none());
        assert!(check.refresh.is_none());
        assert!(!path.exists());
    }

    #[test]
    fn fresh_cache_needs_no_refresh() {
        let tmpdir = create_temp_dir().unwrap();
        let path = tmpdir.path().join("version.json");
        update_version_cache(&path, |cache| {
            cache.latest_version = "v99.0.0".to_string();
            cache.last_checked_at = now_unix_seconds();
        })
        .unwrap();
        let check = start_update_check(allowing(), Some(path.clone()));
        assert_eq!(check.cache_path.as_deref(), Some(path.as_path()));
        assert!(check.refresh.is_none());
    }

    #[test]
    fn old_cache_files_without_last_notified_at_still_load() {
        let tmpdir = create_temp_dir().unwrap();
        let path = tmpdir.path().join("version.json");
        std::fs::write(
            &path,
            "{\"latest_version\":\"v1.2.3\",\"last_checked_at\":10}\n",
        )
        .unwrap();
        let cache = read_version_cache(&path).unwrap();
        assert_eq!(cache.latest_version, "v1.2.3");
        assert_eq!(cache.last_notified_at, None);

        update_version_cache(&path, |cache| cache.last_notified_at = Some(20)).unwrap();
        let cache = read_version_cache(&path).unwrap();
        assert_eq!(cache.latest_version, "v1.2.3");
        assert_eq!(cache.last_checked_at, 10);
        assert_eq!(cache.last_notified_at, Some(20));
    }

    fn cache_with(latest: &str, notified: Option<i64>) -> VersionCache {
        VersionCache {
            latest_version: latest.to_string(),
            last_checked_at: 0,
            last_notified_at: notified,
        }
    }

    #[test]
    fn notice_names_versions_and_upgrade_command() {
        let notice = update_notice(
            &cache_with("v0.12.0", None),
            "0.11.1",
            1_000,
            "hyperliquid update",
        );
        assert_eq!(
            notice.as_deref(),
            Some(
                "hyperliquid 0.12.0 is available. You have 0.11.1.\nUpgrade:\n    hyperliquid update"
            )
        );
    }

    #[test]
    fn notice_shows_at_most_once_per_day() {
        let now = 10 * 24 * 60 * 60;
        let due = |notified| {
            update_notice(&cache_with("v0.12.0", Some(notified)), "0.11.1", now, "x").is_some()
        };
        assert!(!due(now - 60));
        assert!(!due(now - 24 * 60 * 60 + 1));
        assert!(due(now - 24 * 60 * 60));
        assert!(
            due(now + 60),
            "a notification timestamp in the future is ignored"
        );
    }

    #[test]
    fn notice_needs_a_newer_release() {
        assert!(update_notice(&cache_with("v0.11.1", None), "0.11.1", 0, "x").is_none());
        assert!(update_notice(&cache_with("v0.10.0", None), "0.11.1", 0, "x").is_none());
        assert!(update_notice(&cache_with("", None), "0.11.1-rc.1", 0, "x").is_none());
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

    #[test]
    fn update_binary_is_written_with_create_new() {
        let tmpdir = create_temp_dir().unwrap();
        let source = tmpdir.path().join("source");
        std::fs::write(&source, b"binary").unwrap();
        let destination = tmpdir.path().join("destination");

        copy_to_new_executable(&source, &destination).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&destination)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode & 0o700, 0o700);
        }

        let err = copy_to_new_executable(&source, &destination).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&destination).unwrap(), b"binary");
    }
}
