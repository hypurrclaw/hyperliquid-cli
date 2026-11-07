//! The OWS vault: wallet lifecycle and default-wallet selection.
//!
//! A [`Vault`] owns the vault path and passphrase, read from settings once, so
//! callers never thread a path or re-read `OWS_PASSPHRASE`.

use std::path::{Path, PathBuf};

use hypersdk::Address;
use ows_lib::types::{AccountInfo, WalletInfo};

use super::{EVM_CAIP2, HYPERLIQUID_CAIP2, HYPERLIQUID_CHAIN_ALIAS, map_ows_error};
use crate::errors::CliError;
use crate::settings::{Settings, env};

/// Printed with errors that need the user to pick a default wallet.
const SET_DEFAULT_HINT: &str = "hyperliquid account set-default <WALLET>";

#[derive(Clone, Default)]
pub struct Vault {
    path: Option<PathBuf>,
    passphrase: Option<String>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("path", &self.path)
            .field(
                "passphrase",
                &self.passphrase.as_ref().map(|_| "[redacted]"),
            )
            .finish()
    }
}

/// A wallet's account on the chain used for Hyperliquid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperliquidAccount {
    /// The address exactly as the vault stores it.
    pub address_text: String,
    pub address: Address,
    pub chain_id: String,
}

impl Vault {
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            path: settings.ows_vault_path(),
            passphrase: settings.ows_passphrase(),
        }
    }

    /// A vault at an explicit directory with no passphrase.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            passphrase: None,
        }
    }

    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The vault directory for messages and receipts.
    #[must_use]
    pub fn display_path(&self) -> String {
        self.path.as_deref().map_or_else(
            || "~/.hyperliquid".to_string(),
            |path| path.display().to_string(),
        )
    }

    #[must_use]
    pub fn passphrase(&self) -> Option<&str> {
        self.passphrase.as_deref()
    }

    /// Warn on stderr when a new wallet will be encrypted with an empty passphrase.
    pub fn warn_if_passphrase_unset(&self) {
        if self.passphrase.is_some() {
            return;
        }
        let vault = self.path.as_deref().map_or_else(
            || "the vault".to_string(),
            |path| path.join("wallets").display().to_string(),
        );
        let passphrase_env = env::OWS_PASSPHRASE;
        eprintln!(
            "! {passphrase_env} isn't set, so this wallet is encrypted with an empty passphrase."
        );
        eprintln!(
            "  Anyone who can read {vault} can recover the key. To use a passphrase, delete the wallet and recreate it with:"
        );
        eprintln!("    {passphrase_env}=<passphrase> hyperliquid wallet create");
    }

    /// Whether the vault's wallet directory exists. Reads must not create it.
    fn exists(&self) -> bool {
        self.path
            .as_deref()
            .is_none_or(|path| path.join("wallets").is_dir())
    }

    fn require_existing(&self, name_or_id: &str) -> Result<(), CliError> {
        if self.exists() {
            Ok(())
        } else {
            Err(CliError::OwsWalletNotFound {
                wallet: name_or_id.to_string(),
            })
        }
    }

    pub fn list(&self) -> Result<Vec<WalletInfo>, CliError> {
        if !self.exists() {
            return Ok(Vec::new());
        }
        ows_lib::list_wallets(self.path()).map_err(map_ows_error)
    }

    /// One wallet by id or name.
    pub fn get(&self, name_or_id: &str) -> Result<WalletInfo, CliError> {
        self.require_existing(name_or_id)?;
        ows_lib::get_wallet(name_or_id, self.path()).map_err(map_ows_error)
    }

    /// Create a wallet with a new 12-word BIP-39 mnemonic.
    pub fn create(&self, name: &str) -> Result<WalletInfo, CliError> {
        ows_lib::create_wallet(name, Some(12), self.passphrase(), self.path())
            .map_err(map_ows_error)
    }

    pub fn import_private_key(
        &self,
        name: &str,
        private_key_hex: &str,
    ) -> Result<WalletInfo, CliError> {
        ows_lib::import_wallet_private_key(
            name,
            private_key_hex,
            Some(HYPERLIQUID_CHAIN_ALIAS),
            self.passphrase(),
            self.path(),
            None,
            None,
        )
        .map_err(map_ows_error)
    }

    pub fn import_mnemonic(&self, name: &str, mnemonic: &str) -> Result<WalletInfo, CliError> {
        ows_lib::import_wallet_mnemonic(name, mnemonic, self.passphrase(), None, self.path())
            .map_err(map_ows_error)
    }

    pub fn delete(&self, name_or_id: &str) -> Result<(), CliError> {
        self.require_existing(name_or_id)?;
        ows_lib::delete_wallet(name_or_id, self.path()).map_err(map_ows_error)
    }

    pub fn rename(&self, name_or_id: &str, new_name: &str) -> Result<(), CliError> {
        self.require_existing(name_or_id)?;
        ows_lib::rename_wallet(name_or_id, new_name, self.path()).map_err(map_ows_error)
    }

    /// The wallet's mnemonic or private key.
    pub fn export(&self, name_or_id: &str) -> Result<String, CliError> {
        self.require_existing(name_or_id)?;
        ows_lib::export_wallet(name_or_id, self.passphrase(), self.path()).map_err(map_ows_error)
    }

    /// The account used for Hyperliquid: explicit `eip155:999`, else the EVM
    /// account `eip155:1`. A wallet with neither is `OwsNoChainAccount`.
    pub fn hyperliquid_account(wallet: &WalletInfo) -> Result<HyperliquidAccount, CliError> {
        let account: &AccountInfo = [HYPERLIQUID_CAIP2, EVM_CAIP2]
            .iter()
            .find_map(|chain| {
                wallet
                    .accounts
                    .iter()
                    .find(|account| account.chain_id == *chain)
            })
            .ok_or_else(|| CliError::OwsNoChainAccount {
                wallet: wallet.name.clone(),
                caip2: HYPERLIQUID_CAIP2.to_string(),
            })?;
        let address = account.address.parse::<Address>().map_err(|_| {
            CliError::InvalidAuth(format!(
                "ows_malformed_response: OWS wallet '{}' returned invalid EVM address '{}'",
                wallet.name, account.address
            ))
        })?;
        Ok(HyperliquidAccount {
            address_text: account.address.clone(),
            address,
            chain_id: account.chain_id.clone(),
        })
    }

    /// The wallet that signs when no signer is named.
    ///
    /// A configured `default_wallet_id` must resolve to a Hyperliquid-capable
    /// wallet; a stale id is an error naming it, never a silent substitution.
    /// With no configured default, the only Hyperliquid-capable wallet in the
    /// vault is used. An empty vault is `AuthRequired`; several wallets with no
    /// default is an error that asks for `account set-default`.
    pub fn default_wallet(&self, default_wallet_id: Option<&str>) -> Result<WalletInfo, CliError> {
        if let Some(id) = default_wallet_id {
            return match self.get(id) {
                Ok(wallet) if Self::hyperliquid_account(&wallet).is_ok() => Ok(wallet),
                Ok(_) => Err(CliError::InvalidAuth(format!(
                    "Default wallet {id} has no Hyperliquid account.\nPick another:\n    {SET_DEFAULT_HINT}"
                ))),
                Err(CliError::OwsWalletNotFound { .. }) => Err(CliError::InvalidAuth(format!(
                    "Default wallet {id} isn't in the vault.\nPick another:\n    {SET_DEFAULT_HINT}"
                ))),
                Err(err) => Err(err),
            };
        }
        let mut wallets = self.hyperliquid_wallets()?;
        match wallets.len() {
            0 => Err(CliError::AuthRequired),
            1 => Ok(wallets.remove(0)),
            count => Err(CliError::InvalidAuth(format!(
                "No default wallet is set and the vault holds {count} wallets.\nPick one:\n    {SET_DEFAULT_HINT}"
            ))),
        }
    }

    /// Wallets that have a Hyperliquid account.
    pub fn hyperliquid_wallets(&self) -> Result<Vec<WalletInfo>, CliError> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|wallet| Self::hyperliquid_account(wallet).is_ok())
            .collect())
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub const KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000009";

    pub fn import_test_wallet(vault_path: &Path, name: &str) -> WalletInfo {
        ows_lib::import_wallet_private_key(
            name,
            KEY,
            Some("hyperliquid"),
            None,
            Some(vault_path),
            None,
            None,
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::import_test_wallet;
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn reads_do_not_create_a_missing_vault() {
        let tmp = TempDir::new().unwrap();
        let vault = Vault::at(tmp.path().join("missing"));
        assert!(vault.list().unwrap().is_empty());
        assert!(matches!(
            vault.get("anything"),
            Err(CliError::OwsWalletNotFound { .. })
        ));
        assert!(!tmp.path().join("missing").exists());
    }

    #[test]
    fn imported_wallet_resolves_through_the_evm_account_fallback() {
        let tmp = TempDir::new().unwrap();
        let wallet = import_test_wallet(tmp.path(), "test-wallet");
        let evm = wallet
            .accounts
            .iter()
            .find(|account| account.chain_id == EVM_CAIP2)
            .expect("imported wallets always carry an EVM account");

        let account = Vault::hyperliquid_account(&wallet).unwrap();

        assert_eq!(account.chain_id, EVM_CAIP2);
        assert_eq!(account.address_text, evm.address);
        assert_eq!(format!("{:?}", account.address), evm.address.to_lowercase());
    }

    #[test]
    fn wallet_without_any_evm_account_is_no_chain_account() {
        let tmp = TempDir::new().unwrap();
        let mut wallet = import_test_wallet(tmp.path(), "test-wallet");
        wallet.accounts.clear();
        assert!(matches!(
            Vault::hyperliquid_account(&wallet),
            Err(CliError::OwsNoChainAccount { .. })
        ));
    }

    #[test]
    fn default_wallet_rules() {
        let tmp = TempDir::new().unwrap();
        let vault = Vault::at(tmp.path());
        assert!(matches!(
            vault.default_wallet(None),
            Err(CliError::AuthRequired)
        ));

        let first = import_test_wallet(tmp.path(), "first");
        assert_eq!(vault.default_wallet(None).unwrap().id, first.id);
        assert_eq!(vault.default_wallet(Some("first")).unwrap().id, first.id);

        let err = vault.default_wallet(Some("gone")).unwrap_err();
        assert!(err.to_string().contains("isn't in the vault"));

        import_test_wallet(tmp.path(), "second");
        let err = vault.default_wallet(None).unwrap_err();
        assert!(err.to_string().contains("holds 2 wallets"));
        assert!(err.to_string().contains(SET_DEFAULT_HINT));
        assert_eq!(vault.default_wallet(Some(&first.id)).unwrap().name, "first");
    }

    #[test]
    fn debug_hides_the_passphrase() {
        let vault = Vault {
            path: None,
            passphrase: Some("hunter2".to_string()),
        };
        assert!(!format!("{vault:?}").contains("hunter2"));
    }
}
