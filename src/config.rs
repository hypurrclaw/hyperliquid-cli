//! The persisted config file.
//!
//! The file is created by `setup` and the wallet commands that choose a default
//! wallet or write builder/referral defaults (`wallet create|import|import-mnemonic|reset`,
//! `account add --default|set-default|remove`). A missing file is not an error.
//! Layering with environment variables and flags happens in [`crate::settings`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::errors::CliError;
use crate::network::Network;
use crate::private_fs::{self, FileLock, LockError};

const LOCK_WAIT: Duration = Duration::from_secs(10);

/// Fields are optional: the file may be empty or partial.
#[derive(Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// Plaintext private key (0x-prefixed hex). Never written by this CLI; read so
    /// files from older versions keep working. Import it into the vault instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,

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

impl Config {
    /// Whether `wallet` (an id or a name) is the configured default wallet.
    #[must_use]
    pub fn is_default_wallet(&self, id: &str, name: &str) -> bool {
        self.default_wallet_id
            .as_deref()
            .is_some_and(|default| default == id || default == name)
    }

    #[must_use]
    pub fn has_builder_default(&self) -> bool {
        let set = |value: &Option<String>| {
            value
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
        };
        set(&self.default_builder_address) || set(&self.default_builder_fee_rate)
    }

    #[must_use]
    pub fn has_referral_default(&self) -> bool {
        self.default_referral_code
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
    }

    pub(crate) fn plaintext_key(&self) -> Option<&str> {
        self.private_key.as_deref().filter(|key| !key.is_empty())
    }

    pub(crate) fn has_plaintext_key(&self) -> bool {
        self.plaintext_key().is_some()
    }
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

/// A config file location. Reads never print; callers decide whether to show
/// [`ConfigFile::hygiene_warnings`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFile {
    path: PathBuf,
}

impl ConfigFile {
    #[must_use]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    #[must_use]
    pub fn default_location() -> Option<Self> {
        crate::paths::config_file().map(Self::at)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// The raw file text. `Ok(None)` when the file doesn't exist.
    pub fn read_text(&self) -> Result<Option<String>, CliError> {
        if !self.path.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(&self.path)
            .map(Some)
            .map_err(|err| {
                CliError::Configuration(format!(
                    "failed to read config file {}: {err}",
                    self.path.display()
                ))
            })
    }

    /// Parse file text as `T`, naming this file in the error.
    pub fn parse_with<T: serde::de::DeserializeOwned>(&self, text: &str) -> Result<T, CliError> {
        serde_json::from_str(text).map_err(|err| {
            CliError::Configuration(format!(
                "invalid config file {}: {err}",
                self.path.display()
            ))
        })
    }

    pub fn parse(&self, text: &str) -> Result<Config, CliError> {
        self.parse_with(text)
    }

    /// `Ok(None)` when the file doesn't exist; an error when it exists but can't be parsed.
    pub fn read(&self) -> Result<Option<Config>, CliError> {
        self.read_text()?.map(|text| self.parse(&text)).transpose()
    }

    /// Stderr lines for a parsed config that stores a plaintext key or is
    /// readable by other users. Empty when the file is fine.
    #[must_use]
    pub fn hygiene_warnings(&self, has_plaintext_key: bool) -> Vec<String> {
        let path = self.path.display();
        let mut lines = Vec::new();
        if has_plaintext_key {
            lines.push(format!(
                "! Config file {path} stores a plaintext private key."
            ));
            lines.push(
                "  Import it into the vault, then remove private_key from the file:".to_string(),
            );
            lines.push("    hyperliquid wallet import".to_string());
        }
        if let Some(mode) = private_fs::loose_mode(&self.path) {
            lines.push(format!(
                "! Config file {path} is readable by other users (mode {mode:04o})."
            ));
            lines.push("  Restrict it:".to_string());
            lines.push(format!("    chmod 600 {path}"));
        }
        lines
    }

    /// Hold the lock for a config read-modify-write so concurrent CLI processes
    /// can't lose each other's keys. Waits up to 10 seconds.
    pub fn lock(&self) -> Result<FileLock, CliError> {
        FileLock::acquire(&self.path, LOCK_WAIT).map_err(|err| match err {
            LockError::Io(err) => CliError::Configuration(format!(
                "failed to lock config file {}: {err}",
                self.path.display()
            )),
            LockError::Busy => CliError::Configuration(format!(
                "Couldn't lock config file {} because another hyperliquid process holds it. Try again once it finishes.",
                self.path.display()
            )),
        })
    }

    /// Replace the file atomically with mode 0600, creating the directory (0700).
    pub fn save(&self, config: &Config) -> Result<(), anyhow::Error> {
        let json = serde_json::to_string_pretty(config)
            .map_err(|err| anyhow::anyhow!("Failed to serialize config: {err}"))?;
        private_fs::write_atomic(&self.path, json.as_bytes()).map_err(|err| {
            anyhow::anyhow!("Failed to write config file {}: {err}", self.path.display())
        })
    }

    /// Delete the file. A file that is already gone is success.
    pub fn remove(&self) -> Result<(), CliError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(CliError::Configuration(format!(
                "failed to remove config {}: {err}",
                self.path.display()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn file_in(tmp: &TempDir) -> ConfigFile {
        ConfigFile::at(
            tmp.path()
                .join("nested")
                .join("hyperliquid")
                .join("config.json"),
        )
    }

    #[test]
    fn save_then_read_roundtrips_and_skips_unset_fields() {
        let tmp = TempDir::new().unwrap();
        let file = file_in(&tmp);
        let config = Config {
            network: Network::Testnet,
            default_wallet_id: Some("wallet-1".to_string()),
            ..Config::default()
        };
        file.save(&config).unwrap();

        assert_eq!(file.read().unwrap(), Some(config));
        let text = std::fs::read_to_string(file.path()).unwrap();
        assert!(!text.contains("private_key"));
        assert!(!text.contains("default_builder"));
    }

    #[test]
    fn missing_file_reads_as_none_and_invalid_json_is_a_configuration_error() {
        let tmp = TempDir::new().unwrap();
        let file = file_in(&tmp);
        assert_eq!(file.read().unwrap(), None);

        file.save(&Config::default()).unwrap();
        std::fs::write(file.path(), "{not json").unwrap();
        let err = file.read().unwrap_err();
        assert!(matches!(err, CliError::Configuration(_)));
        assert!(err.to_string().contains("invalid config file"));
    }

    #[test]
    fn invalid_network_in_file_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let file = file_in(&tmp);
        file.save(&Config::default()).unwrap();
        std::fs::write(file.path(), r#"{"network":"bogus"}"#).unwrap();
        assert!(file.read().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn saved_file_is_owner_only_in_owner_only_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let file = file_in(&tmp);
        file.save(&Config::default()).unwrap();

        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(file.path()), 0o600);
        assert_eq!(mode(file.path().parent().unwrap()), 0o700);
        assert!(file.hygiene_warnings(false).is_empty());
    }

    #[test]
    fn hygiene_warns_about_plaintext_key() {
        let tmp = TempDir::new().unwrap();
        let file = file_in(&tmp);
        let config = Config {
            private_key: Some("0xabc".to_string()),
            ..Config::default()
        };
        file.save(&config).unwrap();
        let lines = file.hygiene_warnings(config.has_plaintext_key());
        assert!(lines[0].contains("stores a plaintext private key"));
        assert!(lines.iter().any(|line| line.contains("wallet import")));
    }

    #[test]
    fn lock_excludes_a_second_holder_until_released() {
        let tmp = TempDir::new().unwrap();
        let file = ConfigFile::at(tmp.path().join("config.json"));
        let first = file.lock().unwrap();
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
    fn debug_redacts_private_key() {
        let config = Config {
            private_key: Some("0xsupersecret".to_string()),
            ..Config::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("supersecret"));
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn default_wallet_matches_id_or_name() {
        let config = Config {
            default_wallet_id: Some("main".to_string()),
            ..Config::default()
        };
        assert!(config.is_default_wallet("id-1", "main"));
        assert!(config.is_default_wallet("main", "other"));
        assert!(!config.is_default_wallet("id-1", "other"));
    }
}
