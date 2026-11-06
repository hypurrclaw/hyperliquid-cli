//! Environment variable names and the layered settings built from flags,
//! environment and the config file.
//!
//! Every `HYPERLIQUID_*` name the CLI reads is a constant in [`env`]; the root
//! help text is generated from the same constants. [`Settings`] reads the
//! environment through an [`EnvSource`] and parses the config file at most once,
//! lazily, so a flag that settles a question never touches a broken config file.

use std::collections::HashMap;
use std::ffi::OsString;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::config::{Config, ConfigFile};
use crate::errors::CliError;
use crate::network::Network;

pub mod env {
    pub const PRIVATE_KEY: &str = "HYPERLIQUID_PRIVATE_KEY";
    pub const NETWORK: &str = "HYPERLIQUID_NETWORK";
    pub const API_BASE_URL: &str = "HYPERLIQUID_API_BASE_URL";
    pub const MAINNET_API_BASE_URL: &str = "HYPERLIQUID_MAINNET_API_BASE_URL";
    pub const TESTNET_API_BASE_URL: &str = "HYPERLIQUID_TESTNET_API_BASE_URL";
    pub const KEYSTORE_PASSWORD: &str = "HYPERLIQUID_KEYSTORE_PASSWORD";
    pub const OWS_VAULT_PATH: &str = "HYPERLIQUID_OWS_VAULT_PATH";
    pub const OWS_PASSPHRASE: &str = "OWS_PASSPHRASE";
    pub const ACCOUNT_KEY_PASSPHRASE: &str = "HYPERLIQUID_ACCOUNT_KEY_PASSPHRASE";
    pub const ACCOUNT_KEYCHAIN_DISABLED: &str = "HYPERLIQUID_ACCOUNT_KEYCHAIN_DISABLED";
    /// Removed: it persisted raw account encryption keys. Setting it is an error.
    pub const ACCOUNT_KEY_STORE_DIR: &str = "HYPERLIQUID_ACCOUNT_KEY_STORE_DIR";
    pub const DEFAULT_BUILDER_ADDRESS: &str = "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS";
    pub const DEFAULT_BUILDER_FEE_RATE: &str = "HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE";
    pub const DEFAULT_REFERRAL_CODE: &str = "HYPERLIQUID_DEFAULT_REFERRAL_CODE";
    pub const NO_UPDATE_CHECK: &str = "HYPERLIQUID_NO_UPDATE_CHECK";
    pub const NO_UPDATE_NOTIFIER: &str = "NO_UPDATE_NOTIFIER";
    pub const CARGO_HOME: &str = "CARGO_HOME";
}

/// Compile-time builder default captured from the build environment.
///
/// Set `HYPERLIQUID_DEFAULT_BUILDER_ADDRESS` while building the binary to bake
/// in a default builder address. A runtime variable with the same name still
/// overrides it. Empty by default in the upstream repo. `option_env!` needs the
/// literal name, which must match [`env::DEFAULT_BUILDER_ADDRESS`].
pub const BUILD_DEFAULT_BUILDER_ADDRESS: &str =
    match option_env!("HYPERLIQUID_DEFAULT_BUILDER_ADDRESS") {
        Some(address) => address,
        None => "",
    };

/// Compile-time default builder fee rate, a percent string such as `0.001%`.
/// Pairs with [`BUILD_DEFAULT_BUILDER_ADDRESS`]; see [`env::DEFAULT_BUILDER_FEE_RATE`].
pub const BUILD_DEFAULT_BUILDER_FEE_RATE: &str =
    match option_env!("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE") {
        Some(rate) => rate,
        None => "",
    };

/// Compile-time default referral code. A runtime variable with the same name
/// still overrides it. Empty by default in the upstream repo, meaning no mainnet
/// default. See [`env::DEFAULT_REFERRAL_CODE`].
pub const BUILD_DEFAULT_REFERRAL_CODE: &str = match option_env!("HYPERLIQUID_DEFAULT_REFERRAL_CODE")
{
    Some(code) => code,
    None => "",
};

const TESTNET_REFERRAL_FALLBACK: &str = "TESTNET";

/// The `Environment:` section of the root help, in display order. Names owned
/// by other modules appear here as plain text.
const ENV_HELP: &[(&str, &str)] = &[
    (
        "HYPERLIQUID_FORMAT",
        "Default output format: pretty, table, or json. --format overrides it.",
    ),
    (
        "HYPERLIQUID_AGENT",
        "Set to 1 for JSON output and no prompts.",
    ),
    (
        env::NETWORK,
        "Network to use: mainnet or testnet. --testnet overrides it.",
    ),
    (
        env::PRIVATE_KEY,
        "Private key for signing. Prefer it over --private-key.",
    ),
    (
        env::KEYSTORE_PASSWORD,
        "Password for --keystore. Prefer it over --keystore-password.",
    ),
    (
        env::API_BASE_URL,
        "Override the API endpoint. Loopback hosts only, for testing.",
    ),
    (env::MAINNET_API_BASE_URL, "Same override, mainnet only."),
    (env::TESTNET_API_BASE_URL, "Same override, testnet only."),
    (
        env::OWS_VAULT_PATH,
        "OWS vault directory. Defaults to ~/.hyperliquid.",
    ),
    (env::OWS_PASSPHRASE, "Passphrase for the OWS vault."),
    (
        env::NO_UPDATE_CHECK,
        "Set to 1 to skip release update checks.",
    ),
    (
        "HYPERLIQUID_WATCH_MAX_TICKS",
        "Tick bound for --watch when --max-ticks is not given.",
    ),
    (
        "HYPERLIQUID_SUBSCRIBE_MAX_EVENTS",
        "Event bound for subscribe when no bound option is given.",
    ),
    ("NO_COLOR", "Disable color when set to a non-empty value."),
    (
        "FORCE_COLOR",
        "Force color on, or off when set to 0 or false.",
    ),
    ("CI", "Treat the run as non-interactive: no prompts."),
];

/// The `Environment:` block of the root help text.
#[must_use]
pub fn env_help_section() -> String {
    let width = ENV_HELP
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or_default()
        + 2;
    let mut section = String::from("Environment:\n");
    for (name, description) in ENV_HELP {
        section.push_str(&format!("  {name:<width$}{description}\n"));
    }
    section
}

/// Where environment variables come from. Tests inject a fixed map instead of
/// mutating the process environment.
#[derive(Debug, Clone, Default)]
pub enum EnvSource {
    #[default]
    Process,
    Fixed(HashMap<String, OsString>),
}

impl EnvSource {
    pub fn fixed<K, V>(pairs: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        V: Into<OsString>,
    {
        Self::Fixed(
            pairs
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        )
    }

    fn get_os(&self, name: &str) -> Option<OsString> {
        match self {
            Self::Process => std::env::var_os(name),
            Self::Fixed(map) => map.get(name).cloned(),
        }
    }

    /// The value when set and valid Unicode.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<String> {
        self.get_os(name).and_then(|value| value.into_string().ok())
    }

    /// The value when set and non-empty.
    #[must_use]
    pub fn get_non_empty(&self, name: &str) -> Option<String> {
        self.get(name).filter(|value| !value.is_empty())
    }

    /// The trimmed value when set and non-blank.
    #[must_use]
    pub fn get_trimmed(&self, name: &str) -> Option<String> {
        self.get(name)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    /// Whether the variable is set at all, even to an empty or non-Unicode value.
    #[must_use]
    pub fn is_set(&self, name: &str) -> bool {
        self.get_os(name).is_some()
    }

    /// The value with invalid Unicode replaced, when set.
    #[must_use]
    pub fn get_lossy(&self, name: &str) -> Option<String> {
        self.get_os(name)
            .map(|value| value.to_string_lossy().into_owned())
    }
}

/// A builder address/fee pair from one settings layer, not yet validated.
/// The labels name the layer's fields in validation errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderDefaultSetting {
    pub address: Option<String>,
    pub fee_rate: Option<String>,
    pub address_label: &'static str,
    pub fee_label: &'static str,
}

enum ConfigSource {
    File(ConfigFile),
    Preloaded(Option<Config>),
    Absent,
}

/// Flags, environment and config file, resolved lazily.
pub struct Settings {
    env: EnvSource,
    source: ConfigSource,
    announce_hygiene: bool,
    file_text: OnceLock<Result<Option<String>, String>>,
    config: OnceLock<Result<Option<Config>, String>>,
    hygiene_announced: OnceLock<()>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings").finish_non_exhaustive()
    }
}

#[derive(serde::Deserialize)]
struct PrivateKeyOnly {
    private_key: Option<String>,
}

impl Settings {
    /// The real environment and config file. Config hygiene warnings (plaintext
    /// key, loose permissions) print to stderr the first time the file is parsed.
    #[must_use]
    pub fn from_process() -> Self {
        Self::build(
            EnvSource::Process,
            ConfigFile::default_location().map_or(ConfigSource::Absent, ConfigSource::File),
            true,
        )
    }

    /// Settings over `env` and an optional config file. Never prints hygiene warnings.
    #[must_use]
    pub fn new(env: EnvSource, file: Option<ConfigFile>) -> Self {
        Self::build(
            env,
            file.map_or(ConfigSource::Absent, ConfigSource::File),
            false,
        )
    }

    /// Settings over `env` with an in-memory config and no file.
    #[must_use]
    pub fn detached(env: EnvSource, config: Option<Config>) -> Self {
        Self::build(env, ConfigSource::Preloaded(config), false)
    }

    /// The same settings without hygiene warnings, for secondary readers in a
    /// process whose primary settings already announced them.
    #[must_use]
    pub fn quiet(mut self) -> Self {
        self.announce_hygiene = false;
        self
    }

    fn build(env: EnvSource, source: ConfigSource, announce_hygiene: bool) -> Self {
        Self {
            env,
            source,
            announce_hygiene,
            file_text: OnceLock::new(),
            config: OnceLock::new(),
            hygiene_announced: OnceLock::new(),
        }
    }

    #[must_use]
    pub fn env(&self) -> &EnvSource {
        &self.env
    }

    #[must_use]
    pub fn config_file(&self) -> Option<&ConfigFile> {
        match &self.source {
            ConfigSource::File(file) => Some(file),
            ConfigSource::Preloaded(_) | ConfigSource::Absent => None,
        }
    }

    /// The config file for a read-modify-write. Errors when the platform has no config directory.
    pub fn require_config_file(&self) -> Result<&ConfigFile, anyhow::Error> {
        self.config_file()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))
    }

    /// Read the config again, bypassing the cached parse. Use it under the config
    /// lock, where another process may have written since the first read.
    pub fn read_config_fresh(&self) -> Result<Option<Config>, CliError> {
        match &self.source {
            ConfigSource::File(file) => {
                let config = file.read()?;
                if let Some(config) = &config {
                    self.note_parsed(file, config.has_plaintext_key());
                }
                Ok(config)
            }
            ConfigSource::Preloaded(config) => Ok(config.clone()),
            ConfigSource::Absent => Ok(None),
        }
    }

    fn note_parsed(&self, file: &ConfigFile, has_plaintext_key: bool) {
        if !self.announce_hygiene {
            return;
        }
        self.hygiene_announced.get_or_init(|| {
            for line in file.hygiene_warnings(has_plaintext_key) {
                eprintln!("{line}");
            }
        });
    }

    fn file_text(&self, file: &ConfigFile) -> Result<Option<&str>, CliError> {
        self.file_text
            .get_or_init(|| file.read_text().map_err(configuration_message))
            .as_ref()
            .map(Option::as_deref)
            .map_err(|message| CliError::Configuration(message.clone()))
    }

    /// The parsed config file. `Ok(None)` when there is no file.
    pub fn config(&self) -> Result<Option<&Config>, CliError> {
        let file = match &self.source {
            ConfigSource::File(file) => file,
            ConfigSource::Preloaded(config) => return Ok(config.as_ref()),
            ConfigSource::Absent => return Ok(None),
        };
        self.config
            .get_or_init(|| {
                let text = self.file_text(file).map_err(configuration_message)?;
                let config = text.map(|text| file.parse(text)).transpose();
                let config = config.map_err(configuration_message)?;
                if let Some(config) = &config {
                    self.note_parsed(file, config.has_plaintext_key());
                }
                Ok(config)
            })
            .as_ref()
            .map(Option::as_ref)
            .map_err(|message| CliError::Configuration(message.clone()))
    }

    /// The config as the defaults resolvers see it: parse failures read as
    /// "failed to load config".
    fn config_for_defaults(&self) -> Result<Option<&Config>, CliError> {
        self.config()
            .map_err(|err| CliError::Configuration(format!("failed to load config: {err}")))
    }

    fn file_private_key(&self) -> Result<Option<String>, CliError> {
        let file = match &self.source {
            ConfigSource::File(file) => file,
            ConfigSource::Preloaded(config) => {
                return Ok(config
                    .as_ref()
                    .and_then(|config| config.plaintext_key().map(str::to_string)));
            }
            ConfigSource::Absent => return Ok(None),
        };
        let Some(text) = self.file_text(file)? else {
            return Ok(None);
        };
        // Only this field is read, so an unrelated bad field such as `network`
        // doesn't block a run that signs with the key.
        let parsed: PrivateKeyOnly = file.parse_with(text)?;
        let key = parsed.private_key.filter(|key| !key.is_empty());
        self.note_parsed(file, key.is_some());
        Ok(key)
    }

    /// Whether to use testnet. Flag, then `HYPERLIQUID_NETWORK`, then the config file; mainnet by default.
    pub fn testnet(&self, cli_testnet: bool) -> Result<bool, CliError> {
        if cli_testnet {
            return Ok(true);
        }
        if let Some(network) = self.env.get(env::NETWORK) {
            return Ok(Network::parse_setting(env::NETWORK, &network)?.is_testnet());
        }
        Ok(self
            .config()?
            .map(|config| config.network)
            .unwrap_or_default()
            .is_testnet())
    }

    pub fn network(&self, cli_testnet: bool) -> Result<Network, CliError> {
        self.testnet(cli_testnet).map(Network::from_testnet)
    }

    /// The signing private key. Flag, then `HYPERLIQUID_PRIVATE_KEY`, then the
    /// legacy plaintext `private_key` in the config file. Empty values are skipped.
    pub fn private_key(&self, cli_key: Option<&str>) -> Result<Option<String>, CliError> {
        if let Some(key) = cli_key.filter(|key| !key.is_empty()) {
            return Ok(Some(key.to_string()));
        }
        if let Some(key) = self.env.get_non_empty(env::PRIVATE_KEY) {
            return Ok(Some(key));
        }
        self.file_private_key()
    }

    /// The loopback-only API endpoint override for `network`. The network-specific
    /// variable wins over `HYPERLIQUID_API_BASE_URL`.
    pub fn api_base_url_override(
        &self,
        network: Network,
    ) -> Result<Option<reqwest::Url>, CliError> {
        let network_var = match network {
            Network::Mainnet => env::MAINNET_API_BASE_URL,
            Network::Testnet => env::TESTNET_API_BASE_URL,
        };
        for name in [network_var, env::API_BASE_URL] {
            if let Some(url) = parse_api_override(name, self.env.get(name).as_deref())? {
                return Ok(Some(url));
            }
        }
        Ok(None)
    }

    /// The URL the API client talks to: the override, else the public endpoint.
    pub fn api_base_url(&self, network: Network) -> Result<String, CliError> {
        Ok(self
            .api_base_url_override(network)?
            .map_or_else(|| network.api_base_url().to_string(), |url| url.to_string()))
    }

    pub fn default_wallet_id(&self) -> Result<Option<&str>, CliError> {
        Ok(self
            .config()?
            .and_then(|config| config.default_wallet_id.as_deref()))
    }

    pub fn ows_vault_path(&self) -> Option<PathBuf> {
        self.env
            .get_non_empty(env::OWS_VAULT_PATH)
            .map(PathBuf::from)
            .or_else(crate::paths::default_vault_dir)
    }

    pub fn ows_passphrase(&self) -> Option<String> {
        self.env.get_non_empty(env::OWS_PASSPHRASE)
    }

    pub fn keystore_password(&self) -> Option<String> {
        self.env.get_non_empty(env::KEYSTORE_PASSWORD)
    }

    fn env_builder_default(&self) -> Option<BuilderDefaultSetting> {
        let address = self.env.get_trimmed(env::DEFAULT_BUILDER_ADDRESS);
        let fee_rate = self.env.get_trimmed(env::DEFAULT_BUILDER_FEE_RATE);
        (address.is_some() || fee_rate.is_some()).then_some(BuilderDefaultSetting {
            address,
            fee_rate,
            address_label: env::DEFAULT_BUILDER_ADDRESS,
            fee_label: env::DEFAULT_BUILDER_FEE_RATE,
        })
    }

    fn layered_builder_default(&self, config: Option<&Config>) -> Option<BuilderDefaultSetting> {
        if let Some(setting) = self.env_builder_default() {
            return Some(setting);
        }
        if let Some(config) = config {
            let trimmed = |value: &Option<String>| {
                value
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            };
            let address = trimmed(&config.default_builder_address);
            let fee_rate = trimmed(&config.default_builder_fee_rate);
            if address.is_some() || fee_rate.is_some() {
                return Some(BuilderDefaultSetting {
                    address,
                    fee_rate,
                    address_label: "default_builder_address",
                    fee_label: "default_builder_fee_rate",
                });
            }
        }
        let non_empty = |value: &str| (!value.is_empty()).then(|| value.to_string());
        let address = non_empty(BUILD_DEFAULT_BUILDER_ADDRESS);
        let fee_rate = non_empty(BUILD_DEFAULT_BUILDER_FEE_RATE);
        (address.is_some() || fee_rate.is_some()).then_some(BuilderDefaultSetting {
            address,
            fee_rate,
            address_label: "DEFAULT_BUILDER_ADDRESS",
            fee_label: "DEFAULT_BUILDER_FEE_RATE",
        })
    }

    /// The default builder pair: environment, then config file, then the build-time default.
    /// The config file is only read when the environment sets neither value.
    pub fn default_builder(&self) -> Result<Option<BuilderDefaultSetting>, CliError> {
        if let Some(setting) = self.env_builder_default() {
            return Ok(Some(setting));
        }
        let config = self.config_for_defaults()?;
        Ok(self.layered_builder_default(config))
    }

    /// The builder pair setup and wallet creation would offer: environment, then
    /// the build-time default. The config file is the thing being filled in, so it is skipped.
    #[must_use]
    pub fn suggested_default_builder(&self) -> Option<BuilderDefaultSetting> {
        self.layered_builder_default(None)
    }

    fn layered_referral_code(&self, is_testnet: bool, config: Option<&Config>) -> Option<String> {
        if let Some(code) = self.env.get_trimmed(env::DEFAULT_REFERRAL_CODE) {
            return Some(code);
        }
        if let Some(code) = config
            .and_then(|config| config.default_referral_code.as_deref())
            .map(str::trim)
            .filter(|code| !code.is_empty())
        {
            return Some(code.to_string());
        }
        if !BUILD_DEFAULT_REFERRAL_CODE.is_empty() {
            return Some(BUILD_DEFAULT_REFERRAL_CODE.to_string());
        }
        is_testnet.then(|| TESTNET_REFERRAL_FALLBACK.to_string())
    }

    /// The default referral code: environment, config file, build-time default,
    /// then `TESTNET` on testnet. `None` on mainnet means the caller needs an explicit code.
    pub fn default_referral_code(&self, is_testnet: bool) -> Result<Option<String>, CliError> {
        let config = self.config_for_defaults()?;
        Ok(self.layered_referral_code(is_testnet, config))
    }

    /// The referral code setup and wallet creation would offer, without the config file.
    #[must_use]
    pub fn suggested_default_referral_code(&self, is_testnet: bool) -> Option<String> {
        self.layered_referral_code(is_testnet, None)
    }
}

fn configuration_message(err: CliError) -> String {
    match err {
        CliError::Configuration(message) => message,
        other => other.to_string(),
    }
}

/// Parse a hidden API endpoint override. Overrides are limited to loopback hosts
/// so tests can point the CLI at a local mock without routing real trading or
/// market-data traffic through an arbitrary proxy. `name` is the variable that
/// supplied `value`.
fn parse_api_override(name: &str, value: Option<&str>) -> Result<Option<reqwest::Url>, CliError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let url = reqwest::Url::parse(value)
        .map_err(|err| CliError::Configuration(format!("{name} must be a valid URL: {err}")))?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(CliError::Configuration(format!(
            "{name} must use http or https"
        )));
    }

    let host = url
        .host_str()
        .ok_or_else(|| CliError::Configuration(format!("{name} must include a host")))?;

    let is_loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());

    if !is_loopback {
        return Err(CliError::Configuration(format!(
            "{name} is only supported for localhost/loopback test endpoints"
        )));
    }

    Ok(Some(url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const FILE_KEY: &str = "0x00000000000000000000000000000000000000000000000000000000000000f1";

    fn settings(env: &[(&str, &str)], config: Option<Config>) -> Settings {
        Settings::detached(EnvSource::fixed(env.iter().copied()), config)
    }

    fn testnet_config() -> Config {
        Config {
            network: Network::Testnet,
            ..Config::default()
        }
    }

    fn file_settings(env: &[(&str, &str)], contents: &str) -> (TempDir, Settings) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.json");
        std::fs::write(&path, contents).unwrap();
        let settings = Settings::new(
            EnvSource::fixed(env.iter().copied()),
            Some(ConfigFile::at(path)),
        );
        (tmp, settings)
    }

    #[test]
    fn network_precedence_is_flag_then_env_then_file_then_mainnet() {
        let from_file = || Some(testnet_config());
        assert!(settings(&[], None).testnet(true).unwrap());
        assert!(!settings(&[], None).testnet(false).unwrap());
        assert!(settings(&[], from_file()).testnet(false).unwrap());
        assert!(
            !settings(&[(env::NETWORK, "mainnet")], from_file())
                .testnet(false)
                .unwrap()
        );
        assert!(
            settings(&[(env::NETWORK, "TestNet")], None)
                .testnet(false)
                .unwrap()
        );
        assert!(
            settings(&[(env::NETWORK, "bogus")], from_file())
                .testnet(true)
                .unwrap()
        );
    }

    #[test]
    fn invalid_network_env_names_the_variable() {
        let err = settings(&[(env::NETWORK, "bogus")], None)
            .testnet(false)
            .unwrap_err();
        assert!(matches!(err, CliError::Configuration(_)));
        assert!(err.to_string().contains(env::NETWORK));
        assert!(err.to_string().contains("'bogus'"));
    }

    #[test]
    fn invalid_network_in_file_is_a_configuration_error() {
        let (_tmp, settings) = file_settings(&[], r#"{"network":"bogus"}"#);
        let err = settings.testnet(false).unwrap_err();
        assert!(err.to_string().contains("invalid config file"));
        assert!(err.to_string().contains("bogus"));
    }

    #[test]
    fn private_key_precedence_skips_empty_values() {
        let from_file = || {
            Some(Config {
                private_key: Some(FILE_KEY.to_string()),
                ..Config::default()
            })
        };
        let resolve = |env: &[(&str, &str)], cli: Option<&str>| {
            settings(env, from_file()).private_key(cli).unwrap()
        };
        assert_eq!(
            resolve(&[(env::PRIVATE_KEY, "env")], Some("cli")).unwrap(),
            "cli"
        );
        assert_eq!(resolve(&[(env::PRIVATE_KEY, "env")], None).unwrap(), "env");
        assert_eq!(
            resolve(&[(env::PRIVATE_KEY, "env")], Some("")).unwrap(),
            "env"
        );
        assert_eq!(resolve(&[(env::PRIVATE_KEY, "")], None).unwrap(), FILE_KEY);
        assert_eq!(settings(&[], None).private_key(None).unwrap(), None);
    }

    #[test]
    fn private_key_ignores_unrelated_invalid_file_fields() {
        let (_tmp, settings) = file_settings(
            &[],
            &format!(r#"{{"private_key":"{FILE_KEY}","network":"bogus"}}"#),
        );
        assert_eq!(
            settings.private_key(None).unwrap().as_deref(),
            Some(FILE_KEY)
        );
        assert!(settings.testnet(false).is_err());
    }

    #[test]
    fn flags_settle_questions_without_reading_a_broken_file() {
        let (_tmp, settings) = file_settings(&[(env::PRIVATE_KEY, "env")], "{not json");
        assert!(settings.testnet(true).unwrap());
        assert_eq!(settings.private_key(None).unwrap().as_deref(), Some("env"));
        assert!(settings.testnet(false).is_err());
        assert!(settings.private_key(Some("")).is_ok());
    }

    #[test]
    fn config_is_parsed_once() {
        let (tmp, settings) = file_settings(&[], r#"{"network":"testnet"}"#);
        assert!(settings.testnet(false).unwrap());
        std::fs::write(tmp.path().join("config.json"), r#"{"network":"mainnet"}"#).unwrap();
        assert!(settings.testnet(false).unwrap());
    }

    #[test]
    fn api_override_prefers_the_network_variable_and_allows_only_loopback() {
        let both = settings(
            &[
                (env::API_BASE_URL, "http://127.0.0.1:4011"),
                (env::TESTNET_API_BASE_URL, "http://localhost:4012"),
            ],
            None,
        );
        assert_eq!(
            both.api_base_url_override(Network::Testnet)
                .unwrap()
                .unwrap()
                .as_str(),
            "http://localhost:4012/"
        );
        assert_eq!(
            both.api_base_url_override(Network::Mainnet)
                .unwrap()
                .unwrap()
                .as_str(),
            "http://127.0.0.1:4011/"
        );
        assert_eq!(
            settings(&[], None).api_base_url(Network::Mainnet).unwrap(),
            "https://api.hyperliquid.xyz"
        );
    }

    #[test]
    fn api_override_errors_name_the_variable_that_supplied_the_value() {
        let err = settings(&[(env::MAINNET_API_BASE_URL, "https://example.com")], None)
            .api_base_url_override(Network::Mainnet)
            .unwrap_err();
        assert!(err.to_string().contains(env::MAINNET_API_BASE_URL));
        assert!(err.to_string().contains("localhost/loopback"));

        let err = settings(&[(env::API_BASE_URL, "ftp://127.0.0.1")], None)
            .api_base_url_override(Network::Mainnet)
            .unwrap_err();
        assert!(err.to_string().contains(env::API_BASE_URL));
        assert!(err.to_string().contains("http or https"));
    }

    #[test]
    fn builder_default_layers_env_then_config() {
        let address = "0x0000000000000000000000000000000000000abc";
        let config = Config {
            default_builder_address: Some(address.to_string()),
            default_builder_fee_rate: Some("0.001%".to_string()),
            ..Config::default()
        };
        let from_config = settings(&[], Some(config.clone()))
            .default_builder()
            .unwrap()
            .unwrap();
        assert_eq!(from_config.address.as_deref(), Some(address));
        assert_eq!(from_config.address_label, "default_builder_address");

        let from_env = settings(
            &[
                (env::DEFAULT_BUILDER_ADDRESS, "  0xenv "),
                (env::DEFAULT_BUILDER_FEE_RATE, "0.002%"),
            ],
            Some(config),
        )
        .default_builder()
        .unwrap()
        .unwrap();
        assert_eq!(from_env.address.as_deref(), Some("0xenv"));
        assert_eq!(from_env.address_label, env::DEFAULT_BUILDER_ADDRESS);
    }

    #[test]
    fn builder_suggestion_skips_the_config_file() {
        let config = Config {
            default_builder_address: Some("0xabc".to_string()),
            default_builder_fee_rate: Some("0.001%".to_string()),
            ..Config::default()
        };
        let suggestion = settings(&[], Some(config)).suggested_default_builder();
        assert_eq!(
            suggestion.is_some(),
            !BUILD_DEFAULT_BUILDER_ADDRESS.is_empty() || !BUILD_DEFAULT_BUILDER_FEE_RATE.is_empty()
        );
    }

    #[test]
    fn referral_code_layers_env_config_build_then_testnet() {
        let configured = Config {
            default_referral_code: Some(" FROMFILE ".to_string()),
            ..Config::default()
        };
        assert_eq!(
            settings(
                &[(env::DEFAULT_REFERRAL_CODE, "  CODE  ")],
                Some(configured.clone())
            )
            .default_referral_code(false)
            .unwrap()
            .as_deref(),
            Some("CODE")
        );
        assert_eq!(
            settings(&[], Some(configured))
                .default_referral_code(false)
                .unwrap()
                .as_deref(),
            Some("FROMFILE")
        );
        if BUILD_DEFAULT_REFERRAL_CODE.is_empty() {
            assert_eq!(
                settings(&[], None).default_referral_code(false).unwrap(),
                None
            );
            assert_eq!(
                settings(&[], None)
                    .default_referral_code(true)
                    .unwrap()
                    .as_deref(),
                Some("TESTNET")
            );
        }
    }

    #[test]
    fn help_section_lists_every_documented_variable_aligned() {
        let section = env_help_section();
        assert!(section.starts_with("Environment:\n"));
        for (name, description) in ENV_HELP {
            assert!(section.contains(name));
            assert!(section.contains(description));
        }
        let column = section
            .lines()
            .nth(1)
            .unwrap()
            .find("Default output")
            .unwrap();
        assert!(section.lines().skip(1).all(|line| {
            line.get(..column)
                .is_some_and(|prefix| prefix.ends_with(' '))
        }));
    }
}
