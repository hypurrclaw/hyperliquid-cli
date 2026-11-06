//! Config resolution for the Hyperliquid CLI.
//!
//! Priority chain:
//! 1. CLI flags (`--private-key`, `--testnet`)
//! 2. Environment variables (`HYPERLIQUID_PRIVATE_KEY`, `HYPERLIQUID_NETWORK`)
//! 3. Config file (`~/.config/hyperliquid/config.json`)
//!
//! Config file is only created when the user runs `hyperliquid setup` or
//! `hyperliquid wallet create/import`. Missing config is not an error for
//! read-only commands.

use std::net::IpAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::errors::CliError;

// ── Constants ──────────────────────────────────────────────────────────

pub const ENV_PRIVATE_KEY: &str = "HYPERLIQUID_PRIVATE_KEY";
pub const ENV_NETWORK: &str = "HYPERLIQUID_NETWORK";
pub const ENV_API_BASE_URL: &str = "HYPERLIQUID_API_BASE_URL";
pub const ENV_MAINNET_API_BASE_URL: &str = "HYPERLIQUID_MAINNET_API_BASE_URL";
pub const ENV_TESTNET_API_BASE_URL: &str = "HYPERLIQUID_TESTNET_API_BASE_URL";

// ── Network enum ───────────────────────────────────────────────────────

/// Network selection for Hyperliquid API.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    #[default]
    Mainnet,
    Testnet,
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mainnet => write!(f, "mainnet"),
            Self::Testnet => write!(f, "testnet"),
        }
    }
}

/// Custom case-insensitive deserializer for Network.
impl<'de> serde::Deserialize<'de> for Network {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "mainnet" => Ok(Network::Mainnet),
            "testnet" => Ok(Network::Testnet),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["mainnet", "testnet"],
            )),
        }
    }
}

// ── Config struct ──────────────────────────────────────────────────────

/// Configuration stored in `~/.config/hyperliquid/config.json`.
///
/// Fields are optional — the config file may be empty or partial.
/// Missing fields are filled from env vars or CLI flags via the priority chain.
#[derive(Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// Private key (0x-prefixed hex). Stored for convenience; encrypted storage
    /// is planned for the wallet management feature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,

    /// Network selection (mainnet or testnet).
    #[serde(default)]
    pub network: Network,

    /// Default OWS wallet id or name to use when no explicit signer is specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_wallet_id: Option<String>,

    /// Default builder address to apply to order creation when no explicit builder is provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_builder_address: Option<String>,

    /// Default builder fee rate as a percent string, paired with `default_builder_address`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_builder_fee_rate: Option<String>,

    /// Default referral code to apply when `referral set` is invoked without a code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_referral_code: Option<String>,
}

/// Never prints the plaintext key held in `private_key`.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field(
                "private_key",
                &self.private_key.as_ref().map(|_| "[redacted]"),
            )
            .field("network", &self.network)
            .field("default_wallet_id", &self.default_wallet_id)
            .field("default_builder_address", &self.default_builder_address)
            .field("default_builder_fee_rate", &self.default_builder_fee_rate)
            .field("default_referral_code", &self.default_referral_code)
            .finish()
    }
}

#[derive(serde::Deserialize)]
struct PrivateKeyConfig {
    private_key: Option<String>,
}

// ── Path helpers ───────────────────────────────────────────────────────

/// Returns the config directory path for Hyperliquid CLI.
///
/// Uses the `dirs` crate for platform-correct paths:
/// - macOS: `~/Library/Application Support/hyperliquid`
/// - Linux: `~/.config/hyperliquid`
/// - Windows: `C:\Users\<user>\AppData\Roaming\hyperliquid`
pub fn config_dir() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("hyperliquid"))
}

/// Returns the config file path (`~/.config/hyperliquid/config.json` or platform equivalent).
pub fn config_file_path() -> Option<std::path::PathBuf> {
    config_dir().map(|d| d.join("config.json"))
}

// ── Config file I/O ────────────────────────────────────────────────────

/// Load config from a specific file path.
///
/// Returns an error if the file doesn't exist or contains invalid JSON.
pub fn load_config_from_path(path: &Path) -> Result<Config, anyhow::Error> {
    let contents = std::fs::read_to_string(path).map_err(|e| {
        CliError::Configuration(format!(
            "failed to read config file {}: {}",
            path.display(),
            e
        ))
    })?;
    let config: Config = serde_json::from_str(&contents).map_err(|e| {
        CliError::Configuration(format!("invalid config file {}: {}", path.display(), e))
    })?;
    warn_config_hygiene(
        path,
        config
            .private_key
            .as_deref()
            .is_some_and(|key| !key.is_empty()),
    );
    Ok(config)
}

static CONFIG_HYGIENE_WARNED: AtomicBool = AtomicBool::new(false);

/// Warn once per process when the config file stores a plaintext key or is readable by others.
fn warn_config_hygiene(path: &Path, has_plaintext_key: bool) {
    let loose_mode = loose_file_mode(path);
    if !has_plaintext_key && loose_mode.is_none() {
        return;
    }
    if CONFIG_HYGIENE_WARNED.swap(true, Ordering::Relaxed) {
        return;
    }
    if has_plaintext_key {
        eprintln!(
            "! Config file {} stores a plaintext private key.",
            path.display()
        );
        eprintln!("  Import it into the vault, then remove private_key from the file:");
        eprintln!("    hyperliquid wallet import");
    }
    if let Some(mode) = loose_mode {
        eprintln!(
            "! Config file {} is readable by other users (mode {mode:04o}).",
            path.display()
        );
        eprintln!("  Restrict it:");
        eprintln!("    chmod 600 {}", path.display());
    }
}

#[cfg(unix)]
fn loose_file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).ok()?.permissions().mode() & 0o777;
    (mode & 0o077 != 0).then_some(mode)
}

#[cfg(not(unix))]
fn loose_file_mode(_path: &Path) -> Option<u32> {
    None
}

/// Load config from the default path (`~/.config/hyperliquid/config.json`).
///
/// Returns `None` if the file doesn't exist (not an error for read-only commands).
/// Returns an error if the file exists but contains invalid JSON.
pub fn load_config() -> Result<Option<Config>, anyhow::Error> {
    let Some(path) = config_file_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(load_config_from_path(&path)?))
}

/// Create `dir` and any missing parents; new directories are owner-only (0700).
pub fn create_private_dir_all(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

/// Replace `path` with `contents` atomically and owner-only.
///
/// The bytes go to a temp file created with `create_new` and mode 0600 in the
/// target directory, are fsynced, then renamed over `path`. A crash leaves
/// either the old file or the new one, never a truncated file. Missing parent
/// directories are created with mode 0700.
pub fn write_private_file_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    create_private_dir_all(parent)?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());

    let mut attempt = 0_u32;
    let (tmp_path, mut tmp) = loop {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.subsec_nanos());
        let tmp_path = parent.join(format!(
            ".{file_name}.{}.{nanos}.{attempt}.tmp",
            std::process::id()
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&tmp_path) {
            Ok(file) => break (tmp_path, file),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists && attempt < 16 => {
                attempt += 1;
            }
            Err(err) => return Err(err),
        }
    };

    let written = tmp
        .write_all(contents)
        .and_then(|()| tmp.sync_all())
        .and_then(|()| std::fs::rename(&tmp_path, path));
    if let Err(err) = written {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }
    #[cfg(unix)]
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Advisory lock held across a config read-modify-write section.
///
/// Released when dropped.
#[derive(Debug)]
pub struct ConfigLock {
    _file: std::fs::File,
}

/// Lock `<config>.lock` so concurrent CLI processes can't lose each other's keys.
pub fn lock_config() -> Result<ConfigLock, anyhow::Error> {
    let path =
        config_file_path().ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?;
    lock_config_at(&path)
}

/// Lock the sibling `<path>.lock` file, waiting up to 10 seconds for another process.
pub fn lock_config_at(path: &Path) -> Result<ConfigLock, anyhow::Error> {
    let mut lock_name = path.as_os_str().to_os_string();
    lock_name.push(".lock");
    let lock_path = std::path::PathBuf::from(lock_name);
    let io_error = |err: std::io::Error| {
        CliError::Configuration(format!(
            "failed to lock config file {}: {err}",
            path.display()
        ))
    };
    if let Some(parent) = lock_path.parent() {
        create_private_dir_all(parent).map_err(io_error)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&lock_path).map_err(io_error)?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(ConfigLock { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(CliError::Configuration(format!(
                    "Couldn't lock config file {} because another hyperliquid process holds it. Try again once it finishes.",
                    path.display()
                ))
                .into());
            }
            Err(std::fs::TryLockError::Error(err)) => return Err(io_error(err).into()),
        }
    }
}

/// Save config to a specific file path.
///
/// Creates parent directories (0700) if they don't exist and replaces any
/// existing file atomically with mode 0600.
pub fn save_config_to_path(config: &Config, path: &Path) -> Result<(), anyhow::Error> {
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| anyhow::anyhow!("Failed to serialize config: {}", e))?;
    write_private_file_atomic(path, json.as_bytes())
        .map_err(|e| anyhow::anyhow!("Failed to write config file {}: {}", path.display(), e))?;
    Ok(())
}

/// Save config to the default path (`~/.config/hyperliquid/config.json`).
///
/// Creates the config directory if it doesn't exist.
pub fn save_config(config: &Config) -> Result<(), anyhow::Error> {
    let path =
        config_file_path().ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?;
    save_config_to_path(config, &path)
}

// ── Private key resolution ─────────────────────────────────────────────

/// Resolve private key from a config file at the given path.
///
/// Returns `None` if the file doesn't exist or has no `private_key` field.
/// Returns an error if the file exists but cannot be parsed as valid config.
pub fn resolve_private_key_from_file(path: Option<&Path>) -> Result<Option<String>, anyhow::Error> {
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path).map_err(|e| {
        CliError::Configuration(format!(
            "failed to read config file {}: {}",
            path.display(),
            e
        ))
    })?;
    let config: PrivateKeyConfig = serde_json::from_str(&contents).map_err(|e| {
        CliError::Configuration(format!("invalid config file {}: {}", path.display(), e))
    })?;
    let private_key = config.private_key.filter(|key| !key.is_empty());
    warn_config_hygiene(path, private_key.is_some());
    Ok(private_key)
}

/// Resolve private key from CLI flag and environment variable only.
///
/// This is a testable function that doesn't read from the default config file.
/// Priority: CLI flag > env var.
pub fn resolve_private_key_with_env(
    cli_key: Option<&str>,
    env_key: Option<&str>,
) -> Option<String> {
    // 1. CLI flag takes highest priority
    if let Some(key) = cli_key
        && !key.is_empty()
    {
        return Some(key.to_string());
    }

    // 2. Environment variable
    if let Some(key) = env_key
        && !key.is_empty()
    {
        return Some(key.to_string());
    }

    None
}

/// Resolve private key using the full priority chain with explicit parameters.
///
/// Priority: CLI flag > env var > config file path.
/// This is testable without depending on the actual filesystem or env.
pub fn resolve_private_key_full(
    cli_key: Option<&str>,
    env_key: Option<&str>,
    config_path: Option<&Path>,
) -> Result<Option<String>, anyhow::Error> {
    // 1. CLI flag
    if let Some(key) = cli_key
        && !key.is_empty()
    {
        return Ok(Some(key.to_string()));
    }

    // 2. Environment variable
    if let Some(key) = env_key
        && !key.is_empty()
    {
        return Ok(Some(key.to_string()));
    }

    // 3. Config file
    resolve_private_key_from_file(config_path)
}

/// Resolves the private key using the priority chain:
/// CLI flag > env var > config file.
///
/// This is the main entry point that reads from real env vars and the default
/// config file location.
pub fn resolve_private_key(cli_key: Option<&str>) -> Result<Option<String>, anyhow::Error> {
    let env_key = std::env::var(ENV_PRIVATE_KEY).ok();
    let config_path = config_file_path();

    resolve_private_key_full(cli_key, env_key.as_deref(), config_path.as_deref())
}

// ── Network resolution ─────────────────────────────────────────────────

fn parse_network_setting(source: &str, value: &str) -> Result<Network, anyhow::Error> {
    match value.to_lowercase().as_str() {
        "mainnet" => Ok(Network::Mainnet),
        "testnet" => Ok(Network::Testnet),
        _ => Err(CliError::Configuration(format!(
            "{} must be either 'mainnet' or 'testnet' (got '{}')",
            source, value
        ))
        .into()),
    }
}

/// Resolve network from a config file at the given path.
///
/// Returns `None` if the file doesn't exist. If the file exists, it must be
/// valid JSON and any explicit `network` value must be either `mainnet` or
/// `testnet`.
pub fn resolve_network_from_file(path: Option<&Path>) -> Result<Option<Network>, anyhow::Error> {
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let config = load_config_from_path(path)?;
    Ok(Some(config.network))
}

/// Resolve testnet from a config file at the given path.
///
/// Returns `false` if the file doesn't exist or has no `network` field.
/// Returns an error if the file exists but cannot be parsed as valid config.
pub fn resolve_testnet_from_file(path: Option<&Path>) -> Result<bool, anyhow::Error> {
    Ok(resolve_network_from_file(path)?.unwrap_or_default() == Network::Testnet)
}

/// Resolve testnet from CLI flag and environment variable only.
///
/// Priority: CLI flag > env var.
pub fn resolve_testnet_with_env(
    cli_testnet: bool,
    env_network: Option<&str>,
) -> Result<bool, anyhow::Error> {
    // 1. CLI flag
    if cli_testnet {
        return Ok(true);
    }

    // 2. Environment variable
    if let Some(network) = env_network {
        return Ok(parse_network_setting(ENV_NETWORK, network)? == Network::Testnet);
    }

    Ok(false)
}

/// Resolve testnet using the full priority chain with explicit parameters.
///
/// Priority: CLI flag > env var > config file path.
pub fn resolve_testnet_full(
    cli_testnet: bool,
    env_network: Option<&str>,
    config_path: Option<&Path>,
) -> Result<bool, anyhow::Error> {
    // 1. CLI flag
    if cli_testnet {
        return Ok(true);
    }

    // 2. Environment variable
    if let Some(network) = env_network {
        return Ok(parse_network_setting(ENV_NETWORK, network)? == Network::Testnet);
    }

    // 3. Config file
    resolve_testnet_from_file(config_path)
}

/// Resolves whether to use testnet.
///
/// Priority: CLI flag > env var > config file.
/// Defaults to `false` (mainnet).
pub fn resolve_testnet(cli_testnet: bool) -> Result<bool, anyhow::Error> {
    let env_network = std::env::var(ENV_NETWORK).ok();
    let config_path = config_file_path();

    resolve_testnet_full(cli_testnet, env_network.as_deref(), config_path.as_deref())
}

// ── API URL ────────────────────────────────────────────────────────────

/// Returns the API base URL based on network selection.
pub fn api_base_url(testnet: bool) -> &'static str {
    if testnet {
        "https://api.hyperliquid-testnet.xyz"
    } else {
        "https://api.hyperliquid.xyz"
    }
}

/// Resolve the hidden local API endpoint override used by deterministic CLI validation.
///
/// Normal users always get the standard mainnet/testnet endpoints. When set, this override is
/// intentionally constrained to loopback hosts so tests can point the CLI at a local mock without
/// accidentally routing real trading or market-data traffic through arbitrary external proxies.
pub fn resolve_api_base_url_override() -> Result<Option<reqwest::Url>, anyhow::Error> {
    resolve_api_base_url_override_value(std::env::var(ENV_API_BASE_URL).ok().as_deref())
}

/// Resolve a hidden local API endpoint override for the selected network.
///
/// Network-specific overrides are used by deterministic setup validation tests to prove that
/// interactive network selection controls the endpoint being verified. The generic override is
/// retained as a fallback for existing command-level tests.
pub fn resolve_api_base_url_override_for_network(
    network: Network,
) -> Result<Option<reqwest::Url>, anyhow::Error> {
    let network_env_name = match network {
        Network::Mainnet => ENV_MAINNET_API_BASE_URL,
        Network::Testnet => ENV_TESTNET_API_BASE_URL,
    };
    if let Some(url) =
        resolve_api_base_url_override_value(std::env::var(network_env_name).ok().as_deref())?
    {
        return Ok(Some(url));
    }
    resolve_api_base_url_override()
}

fn resolve_api_base_url_override_value(
    value: Option<&str>,
) -> Result<Option<reqwest::Url>, anyhow::Error> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let url = reqwest::Url::parse(value).map_err(|err| {
        CliError::Configuration(format!("{ENV_API_BASE_URL} must be a valid URL: {err}"))
    })?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(
            CliError::Configuration(format!("{ENV_API_BASE_URL} must use http or https")).into(),
        );
    }

    let host = url.host_str().ok_or_else(|| {
        CliError::Configuration(format!("{ENV_API_BASE_URL} must include a host"))
    })?;

    let is_loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());

    if !is_loopback {
        return Err(CliError::Configuration(format!(
            "{ENV_API_BASE_URL} is only supported for localhost/loopback test endpoints"
        ))
        .into());
    }

    Ok(Some(url))
}

// ── Unit tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── Network enum ──────────────────────────────────────────

    #[test]
    fn test_network_display() {
        assert_eq!(Network::Mainnet.to_string(), "mainnet");
        assert_eq!(Network::Testnet.to_string(), "testnet");
    }

    #[test]
    fn test_network_default() {
        assert_eq!(Network::default(), Network::Mainnet);
    }

    #[test]
    fn test_network_serde_roundtrip() {
        let json = serde_json::to_string(&Network::Mainnet).unwrap();
        assert_eq!(json, "\"mainnet\"");
        let parsed: Network = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Network::Mainnet);

        let json = serde_json::to_string(&Network::Testnet).unwrap();
        assert_eq!(json, "\"testnet\"");
        let parsed: Network = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Network::Testnet);
    }

    #[test]
    fn test_network_case_insensitive_deserialize() {
        let parsed: Network = serde_json::from_str("\"TESTNET\"").unwrap();
        assert_eq!(parsed, Network::Testnet);

        let parsed: Network = serde_json::from_str("\"Mainnet\"").unwrap();
        assert_eq!(parsed, Network::Mainnet);
    }

    // ── Config struct ─────────────────────────────────────────

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.private_key.is_none());
        assert_eq!(config.network, Network::Mainnet);
    }

    #[test]
    fn test_config_serde_roundtrip() {
        let config = Config {
            private_key: Some("0xkey".to_string()),
            network: Network::Testnet,
            default_wallet_id: None,
            default_builder_address: None,
            default_builder_fee_rate: None,
            default_referral_code: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.private_key, Some("0xkey".to_string()));
        assert_eq!(parsed.network, Network::Testnet);
    }

    #[test]
    fn test_config_skip_none_private_key() {
        let config = Config {
            private_key: None,
            network: Network::Mainnet,
            default_wallet_id: None,
            default_builder_address: None,
            default_builder_fee_rate: None,
            default_referral_code: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        // "private_key" should not appear since it's None and we skip_serializing_if
        assert!(!json.contains("private_key"));
    }

    // ── Path helpers ──────────────────────────────────────────

    #[test]
    fn test_config_dir_returns_path() {
        let dir = config_dir();
        assert!(dir.is_some());
        let dir = dir.unwrap();
        assert!(dir.to_string_lossy().contains("hyperliquid"));
    }

    #[test]
    fn test_config_file_path_returns_path() {
        let path = config_file_path();
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.to_string_lossy().ends_with("config.json"));
    }

    // ── Config file I/O ───────────────────────────────────────

    #[test]
    fn test_save_and_load_config_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");

        let config = Config {
            private_key: Some("0xabc123".to_string()),
            network: Network::Testnet,
            default_wallet_id: None,
            default_builder_address: None,
            default_builder_fee_rate: None,
            default_referral_code: None,
        };

        save_config_to_path(&config, &config_path).unwrap();
        assert!(config_path.exists());

        let loaded = load_config_from_path(&config_path).unwrap();
        assert_eq!(loaded.private_key, Some("0xabc123".to_string()));
        assert_eq!(loaded.network, Network::Testnet);
    }

    #[test]
    fn test_save_config_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let nested_dir = tmp.path().join("deep").join("nested").join("config");
        let config_path = nested_dir.join("config.json");

        let config = Config {
            private_key: None,
            network: Network::Mainnet,
            default_wallet_id: None,
            default_builder_address: None,
            default_builder_fee_rate: None,
            default_referral_code: None,
        };

        save_config_to_path(&config, &config_path).unwrap();
        assert!(config_path.exists());
    }

    #[test]
    fn test_load_config_missing_file() {
        let result = load_config_from_path(Path::new("/nonexistent/config.json"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_config_invalid_json() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");
        std::fs::write(&config_path, "{invalid json").unwrap();

        let result = load_config_from_path(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_config_default_path_missing() {
        // If default config doesn't exist, load_config returns Ok(None)
        let result = load_config();
        // This may or may not find the real config file, but shouldn't panic
        assert!(result.is_ok());
    }

    // ── Private key resolution ────────────────────────────────

    #[test]
    fn test_resolve_private_key_cli_flag_priority() {
        let result = resolve_private_key_with_env(Some("0xcli_key"), Some("0xenv_key"));
        assert_eq!(result, Some("0xcli_key".to_string()));
    }

    #[test]
    fn test_resolve_private_key_env_fallback() {
        let result = resolve_private_key_with_env(None, Some("0xenv_key"));
        assert_eq!(result, Some("0xenv_key".to_string()));
    }

    #[test]
    fn test_resolve_private_key_none_when_all_empty() {
        let result = resolve_private_key_with_env(None, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_private_key_empty_cli_falls_through() {
        let result = resolve_private_key_with_env(Some(""), Some("0xenv_key"));
        assert_eq!(result, Some("0xenv_key".to_string()));
    }

    #[test]
    fn test_resolve_private_key_empty_env_falls_through() {
        let result = resolve_private_key_with_env(None, Some(""));
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_private_key_from_file() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");

        let config_json = serde_json::json!({
            "private_key": "0xfile_key"
        });
        std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let result = resolve_private_key_from_file(Some(&config_path)).unwrap();
        assert_eq!(result, Some("0xfile_key".to_string()));
    }

    #[test]
    fn test_resolve_private_key_from_missing_file() {
        let result =
            resolve_private_key_from_file(Some(Path::new("/nonexistent/config.json"))).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_private_key_from_none_path() {
        let result = resolve_private_key_from_file(None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_private_key_from_file_empty_key() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");

        let config_json = serde_json::json!({
            "private_key": ""
        });
        std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let result = resolve_private_key_from_file(Some(&config_path)).unwrap();
        assert!(result.is_none());
    }

    // ── Full private key chain ────────────────────────────────

    #[test]
    fn test_full_chain_cli_overrides_env_and_file() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");
        let config_json = serde_json::json!({"private_key": "0xfile_key"});
        std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let resolved =
            resolve_private_key_full(Some("0xcli_key"), Some("0xenv_key"), Some(&config_path))
                .unwrap();
        assert_eq!(resolved, Some("0xcli_key".to_string()));
    }

    #[test]
    fn test_full_chain_env_overrides_file() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");
        let config_json = serde_json::json!({"private_key": "0xfile_key"});
        std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let resolved =
            resolve_private_key_full(None, Some("0xenv_key"), Some(&config_path)).unwrap();
        assert_eq!(resolved, Some("0xenv_key".to_string()));
    }

    #[test]
    fn test_full_chain_file_fallback() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");
        let config_json = serde_json::json!({"private_key": "0xfile_key"});
        std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let resolved = resolve_private_key_full(None, None, Some(&config_path)).unwrap();
        assert_eq!(resolved, Some("0xfile_key".to_string()));
    }

    #[test]
    fn test_full_chain_no_sources() {
        let resolved = resolve_private_key_full(None, None, None).unwrap();
        assert!(resolved.is_none());
    }

    // ── Network resolution ────────────────────────────────────

    #[test]
    fn test_testnet_cli_flag_true() {
        let result = resolve_testnet_with_env(true, Some("mainnet")).unwrap();
        assert!(result);
    }

    #[test]
    fn test_testnet_env_var() {
        let result = resolve_testnet_with_env(false, Some("testnet")).unwrap();
        assert!(result);
    }

    #[test]
    fn test_mainnet_env_var() {
        let result = resolve_testnet_with_env(false, Some("mainnet")).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_testnet_env_case_insensitive() {
        assert!(resolve_testnet_with_env(false, Some("TESTNET")).unwrap());
        assert!(resolve_testnet_with_env(false, Some("TestNet")).unwrap());
    }

    #[test]
    fn test_default_is_mainnet() {
        let result = resolve_testnet_with_env(false, None).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_testnet_flag_overrides_env() {
        let result = resolve_testnet_with_env(true, Some("mainnet")).unwrap();
        assert!(result);
    }

    #[test]
    fn test_testnet_from_config_file() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");
        let config_json = serde_json::json!({"network": "testnet"});
        std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let result = resolve_testnet_from_file(Some(&config_path)).unwrap();
        assert!(result);
    }

    #[test]
    fn test_mainnet_from_config_file() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");
        let config_json = serde_json::json!({"network": "mainnet"});
        std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let result = resolve_testnet_from_file(Some(&config_path)).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_testnet_missing_config_defaults_mainnet() {
        let result = resolve_testnet_from_file(None).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_testnet_from_missing_file() {
        let result =
            resolve_testnet_from_file(Some(Path::new("/nonexistent/config.json"))).unwrap();
        assert!(!result);
    }

    // ── Full network chain ────────────────────────────────────

    #[test]
    fn test_full_network_chain_cli_overrides_env_and_file() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");
        let config_json = serde_json::json!({"network": "mainnet"});
        std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let resolved = resolve_testnet_full(true, Some("mainnet"), Some(&config_path)).unwrap();
        assert!(resolved);
    }

    #[test]
    fn test_full_network_chain_env_overrides_file() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");
        let config_json = serde_json::json!({"network": "mainnet"});
        std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let resolved = resolve_testnet_full(false, Some("testnet"), Some(&config_path)).unwrap();
        assert!(resolved);
    }

    #[test]
    fn test_full_network_chain_env_mainnet_overrides_file_testnet() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");
        let config_json = serde_json::json!({"network": "testnet"});
        std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let resolved = resolve_testnet_full(false, Some("mainnet"), Some(&config_path)).unwrap();
        assert!(!resolved);
    }

    #[test]
    fn test_full_network_chain_file_fallback() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");
        let config_json = serde_json::json!({"network": "testnet"});
        std::fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let resolved = resolve_testnet_full(false, None, Some(&config_path)).unwrap();
        assert!(resolved);
    }

    #[test]
    fn test_full_network_chain_default_mainnet() {
        let resolved = resolve_testnet_full(false, None, None).unwrap();
        assert!(!resolved);
    }

    // ── API URL ───────────────────────────────────────────────

    #[test]
    fn test_api_base_url_mainnet() {
        assert_eq!(api_base_url(false), "https://api.hyperliquid.xyz");
    }

    #[test]
    fn test_api_base_url_testnet() {
        assert_eq!(api_base_url(true), "https://api.hyperliquid-testnet.xyz");
    }

    #[test]
    fn test_api_base_url_override_allows_loopback() {
        let override_url = resolve_api_base_url_override_value(Some("http://127.0.0.1:4011"))
            .unwrap()
            .unwrap();
        assert_eq!(override_url.as_str(), "http://127.0.0.1:4011/");
    }

    #[test]
    fn test_api_base_url_override_rejects_external_hosts() {
        let err = resolve_api_base_url_override_value(Some("https://example.com")).unwrap_err();
        assert!(err.to_string().contains(ENV_API_BASE_URL));
        assert!(err.to_string().contains("localhost/loopback"));
    }

    #[cfg(unix)]
    #[test]
    fn save_config_writes_owner_only_file_in_owner_only_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let path = tmp
            .path()
            .join("nested")
            .join("hyperliquid")
            .join("config.json");

        save_config_to_path(&Config::default(), &path).unwrap();

        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
    }

    #[test]
    fn atomic_write_replaces_contents_and_leaves_no_temp_files() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("secret");

        write_private_file_atomic(&path, b"first").unwrap();
        write_private_file_atomic(&path, b"second").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, vec!["secret".to_string()]);
    }

    #[test]
    fn config_lock_excludes_a_second_holder_until_released() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.json");

        let first = lock_config_at(&path).unwrap();
        let lock_file = std::fs::OpenOptions::new()
            .write(true)
            .open(tmp.path().join("config.json.lock"))
            .unwrap();
        assert!(matches!(
            lock_file.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(first);
        assert!(lock_file.try_lock().is_ok());
    }

    #[test]
    fn config_debug_redacts_private_key() {
        let config = Config {
            private_key: Some("0xsupersecret".to_string()),
            ..Config::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("supersecret"));
        assert!(debug.contains("[redacted]"));
    }
}
