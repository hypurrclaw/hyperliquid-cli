//! The wallet management workflow shared by `wallet`, `account` and `setup`:
//! vault writes paired with config updates, rollback, naming and default markers.
//!
//! Vault failures reach the user as plain messages. Config, lock and
//! wallet-not-found failures keep their typed errors and exit codes.

use ows_lib::types::WalletInfo;

use crate::config::Config;
use crate::errors::CliError;
use crate::ows::vault::{HyperliquidAccount, Vault};
use crate::settings::Settings;

fn vault_failure(err: CliError) -> anyhow::Error {
    anyhow::anyhow!("{err}")
}

/// How a new wallet is obtained.
pub enum NewWallet<'a> {
    Create,
    ImportPrivateKey(&'a str),
    ImportMnemonic(&'a str),
}

/// The vault name for a new wallet.
pub enum WalletName<'a> {
    Exact(&'a str),
    /// The first free name among `base`, `base-2`, `base-3`, ...
    Next(&'a str),
}

/// Edits the config once the wallet exists, under the config lock. Returns the
/// config keys it wrote.
pub type ConfigEdit<'a> =
    Box<dyn FnOnce(&mut Config, &WalletInfo, &HyperliquidAccount) -> Vec<String> + 'a>;

pub struct StoredWallet {
    pub info: WalletInfo,
    pub account: HyperliquidAccount,
    pub config_keys_written: Vec<String>,
}

pub struct WalletStore {
    settings: Settings,
    vault: Vault,
}

impl WalletStore {
    #[must_use]
    pub fn open() -> Self {
        Self::new(Settings::from_process())
    }

    /// A store over `settings`, so a caller that already read the config shares the one parse.
    #[must_use]
    pub fn new(settings: Settings) -> Self {
        let vault = Vault::from_settings(&settings);
        Self { settings, vault }
    }

    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    #[must_use]
    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    pub fn list(&self) -> Result<Vec<WalletInfo>, anyhow::Error> {
        self.vault.list().map_err(vault_failure)
    }

    /// The first free wallet name among `base`, `base-2`, `base-3`, ...
    pub fn next_name(&self, base: &str) -> Result<String, anyhow::Error> {
        let wallets = self.list()?;
        let taken = |name: &str| wallets.iter().any(|wallet| wallet.name == name);
        if !taken(base) {
            return Ok(base.to_string());
        }
        Ok((2..)
            .map(|index| format!("{base}-{index}"))
            .find(|candidate| !taken(candidate))
            .expect("an unbounded range always yields a free name"))
    }

    /// Exact name or id match among `wallets`.
    pub fn find(wallets: Vec<WalletInfo>, selector: &str) -> Result<WalletInfo, CliError> {
        wallets
            .into_iter()
            .find(|wallet| wallet.name == selector || wallet.id == selector)
            .ok_or_else(|| CliError::Unsupported(format!("wallet '{selector}' not found")))
    }

    /// Whether the config names this wallet (by id or name) as its default.
    pub fn is_configured_default(&self, wallet: &WalletInfo) -> Result<bool, CliError> {
        Ok(self
            .settings
            .config()?
            .is_some_and(|config| config.is_default_wallet(&wallet.id, &wallet.name)))
    }

    /// Default markers for Hyperliquid-capable `wallets`: the configured default,
    /// or, with none configured, the vault's only wallet, which is what signing uses.
    pub fn default_markers(&self, wallets: &[WalletInfo]) -> Result<Vec<bool>, CliError> {
        let default_id = self.settings.default_wallet_id()?;
        Ok(wallets
            .iter()
            .map(|wallet| match default_id {
                Some(id) => id == wallet.id || id == wallet.name,
                None => wallets.len() == 1,
            })
            .collect())
    }

    /// Create or import a wallet and, when `config_edit` is given, record it in
    /// the config.
    ///
    /// With an edit, the config is locked and loaded before the wallet exists so
    /// a broken file fails first, and the wallet is removed again if the config
    /// can't be saved.
    pub fn add(
        &self,
        source: &NewWallet<'_>,
        name: &WalletName<'_>,
        config_edit: Option<ConfigEdit<'_>>,
    ) -> Result<StoredWallet, anyhow::Error> {
        self.vault.warn_if_passphrase_unset();

        let editing = match config_edit {
            Some(edit) => {
                let file = self.settings.require_config_file()?;
                let lock = file.lock()?;
                let config = self.settings.read_config_fresh()?.unwrap_or_default();
                Some((file, lock, config, edit))
            }
            None => None,
        };

        let name = match name {
            WalletName::Exact(name) => (*name).to_string(),
            WalletName::Next(base) => self.next_name(base)?,
        };
        let info = match source {
            NewWallet::Create => self.vault.create(&name),
            NewWallet::ImportPrivateKey(key) => self.vault.import_private_key(&name, key),
            NewWallet::ImportMnemonic(mnemonic) => self.vault.import_mnemonic(&name, mnemonic),
        }
        .map_err(vault_failure)?;

        let account = match Vault::hyperliquid_account(&info) {
            Ok(account) => account,
            Err(err) => return Err(self.roll_back(&info, vault_failure(err))),
        };

        let mut config_keys_written = Vec::new();
        if let Some((file, _lock, mut config, edit)) = editing {
            config_keys_written = edit(&mut config, &info, &account);
            if let Err(err) = file.save(&config) {
                return Err(self.roll_back(&info, anyhow::anyhow!("failed to save config: {err}")));
            }
        }

        Ok(StoredWallet {
            info,
            account,
            config_keys_written,
        })
    }

    /// Remove a wallet that was just written. A failed removal is reported with
    /// the original error so the user knows the vault still holds it.
    fn roll_back(&self, wallet: &WalletInfo, cause: anyhow::Error) -> anyhow::Error {
        match self.vault.delete(&wallet.id) {
            Ok(()) => cause,
            Err(err) => anyhow::anyhow!(
                "{cause} (and removing the new wallet '{}' from the vault failed: {err}; delete it with `hyperliquid wallet delete {}`)",
                wallet.name,
                wallet.id
            ),
        }
    }

    /// Make `wallet` the configured default.
    pub fn set_default(&self, wallet: &WalletInfo) -> Result<(), anyhow::Error> {
        let file = self.settings.require_config_file()?;
        let _lock = file.lock()?;
        let mut config = self.settings.read_config_fresh()?.unwrap_or_default();
        config.default_wallet_id = Some(wallet.id.clone());
        file.save(&config)
    }

    /// Delete `wallet` from the vault. When it was the configured default, clear
    /// that reference too.
    pub fn remove(&self, wallet: &WalletInfo, was_default: bool) -> Result<(), anyhow::Error> {
        self.vault.delete(&wallet.id).map_err(vault_failure)?;
        if was_default {
            let file = self.settings.require_config_file()?;
            let _lock = file.lock()?;
            if let Some(mut config) = self.settings.read_config_fresh()? {
                config.default_wallet_id = None;
                file.save(&config)?;
            }
        }
        Ok(())
    }

    pub fn rename(&self, selector: &str, new_name: &str) -> Result<WalletInfo, anyhow::Error> {
        self.vault
            .rename(selector, new_name)
            .map_err(vault_failure)?;
        self.vault.get(new_name).map_err(vault_failure)
    }

    pub fn export(&self, selector: &str) -> Result<String, anyhow::Error> {
        self.vault.export(selector).map_err(vault_failure)
    }

    /// The Hyperliquid account of a stored wallet.
    pub fn account_of(wallet: &WalletInfo) -> Result<HyperliquidAccount, anyhow::Error> {
        Vault::hyperliquid_account(wallet).map_err(vault_failure)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigFile;
    use crate::ows::vault::test_support::{KEY, import_test_wallet};
    use crate::settings::{EnvSource, env};
    use tempfile::TempDir;

    fn store(tmp: &TempDir) -> WalletStore {
        let vault_dir = tmp.path().join("vault");
        let settings = Settings::new(
            EnvSource::fixed([
                (env::OWS_VAULT_PATH, vault_dir.to_str().unwrap()),
                (env::OWS_PASSPHRASE, "test-passphrase"),
            ]),
            Some(ConfigFile::at(
                tmp.path().join("config").join("config.json"),
            )),
        );
        let vault = Vault::from_settings(&settings);
        WalletStore { settings, vault }
    }

    fn saved_config(tmp: &TempDir) -> Config {
        ConfigFile::at(tmp.path().join("config").join("config.json"))
            .read()
            .unwrap()
            .unwrap_or_default()
    }

    fn make_default() -> Option<ConfigEdit<'static>> {
        Some(Box::new(|config, wallet, _| {
            config.default_wallet_id = Some(wallet.id.clone());
            vec!["default_wallet_id".to_string()]
        }))
    }

    #[test]
    fn next_name_skips_taken_names() {
        let tmp = TempDir::new().unwrap();
        let store = store(&tmp);
        assert_eq!(store.next_name("wallet").unwrap(), "wallet");
        import_test_wallet(&tmp.path().join("vault"), "wallet");
        import_test_wallet(&tmp.path().join("vault"), "wallet-2");
        assert_eq!(store.next_name("wallet").unwrap(), "wallet-3");
    }

    #[test]
    fn add_imports_the_wallet_and_records_the_default() {
        let tmp = TempDir::new().unwrap();
        let store = store(&tmp);

        let stored = store
            .add(
                &NewWallet::ImportPrivateKey(KEY),
                &WalletName::Next("imported"),
                make_default(),
            )
            .unwrap();

        assert_eq!(stored.info.name, "imported");
        assert_eq!(stored.config_keys_written, ["default_wallet_id"]);
        assert_eq!(
            saved_config(&tmp).default_wallet_id.as_deref(),
            Some(stored.info.id.as_str())
        );
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn add_without_an_edit_leaves_the_config_alone() {
        let tmp = TempDir::new().unwrap();
        let store = store(&tmp);
        store
            .add(&NewWallet::Create, &WalletName::Exact("plain"), None)
            .unwrap();
        assert!(!tmp.path().join("config").exists());
    }

    #[test]
    fn broken_config_fails_before_a_wallet_exists() {
        let tmp = TempDir::new().unwrap();
        let store = store(&tmp);
        let config_path = tmp.path().join("config").join("config.json");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(&config_path, "{not json").unwrap();

        let err = store
            .add(
                &NewWallet::ImportPrivateKey(KEY),
                &WalletName::Exact("w"),
                make_default(),
            )
            .err()
            .unwrap();

        assert!(err.to_string().contains("invalid config file"));
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn failed_config_save_rolls_the_wallet_back() {
        let tmp = TempDir::new().unwrap();
        let store = store(&tmp);
        let config_path = tmp.path().join("config").join("config.json");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(&config_path, "{}").unwrap();
        // Replacing the file with a directory makes the final rename fail.
        let edit: ConfigEdit<'_> = Box::new(|_, _, _| {
            std::fs::remove_file(&config_path).unwrap();
            std::fs::create_dir(&config_path).unwrap();
            Vec::new()
        });

        let err = store
            .add(
                &NewWallet::ImportPrivateKey(KEY),
                &WalletName::Exact("w"),
                Some(edit),
            )
            .err()
            .unwrap();

        assert!(err.to_string().contains("failed to save config"));
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn find_matches_exact_name_or_id_only() {
        let tmp = TempDir::new().unwrap();
        let wallet = import_test_wallet(tmp.path(), "main");
        let wallets = vec![wallet.clone()];

        assert_eq!(
            WalletStore::find(wallets.clone(), "main").unwrap().id,
            wallet.id
        );
        assert_eq!(
            WalletStore::find(wallets.clone(), &wallet.id).unwrap().name,
            "main"
        );
        let err = WalletStore::find(wallets, "Main").unwrap_err();
        assert_eq!(err.exit_code(), 13);
        assert!(err.to_string().contains("wallet 'Main' not found"));
    }

    #[test]
    fn default_markers_use_the_config_or_the_only_wallet() {
        let tmp = TempDir::new().unwrap();
        let store = store(&tmp);
        let vault_dir = tmp.path().join("vault");
        let first = import_test_wallet(&vault_dir, "first");
        let only = vec![first.clone()];
        assert_eq!(store.default_markers(&only).unwrap(), [true]);

        let second = import_test_wallet(&vault_dir, "second");
        let both = vec![first.clone(), second];
        assert_eq!(store.default_markers(&both).unwrap(), [false, false]);

        let config_file = ConfigFile::at(tmp.path().join("config").join("config.json"));
        config_file
            .save(&Config {
                default_wallet_id: Some("second".to_string()),
                ..Config::default()
            })
            .unwrap();
        let configured = Settings::new(store.settings.env().clone(), Some(config_file));
        let store = WalletStore {
            vault: store.vault.clone(),
            settings: configured,
        };
        assert_eq!(store.default_markers(&both).unwrap(), [false, true]);
        assert!(store.is_configured_default(&both[1]).unwrap());
    }

    #[test]
    fn remove_clears_a_dangling_default() {
        let tmp = TempDir::new().unwrap();
        let store = store(&tmp);
        let stored = store
            .add(
                &NewWallet::ImportPrivateKey(KEY),
                &WalletName::Exact("w"),
                make_default(),
            )
            .unwrap();

        store.remove(&stored.info, true).unwrap();

        assert!(store.list().unwrap().is_empty());
        assert_eq!(saved_config(&tmp).default_wallet_id, None);
    }
}
