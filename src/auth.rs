//! Signer selection: which key signs an authenticated action.
//!
//! One precedence, written once in [`resolve_signer`]:
//! `--ows-signer` > private key (flag, env or legacy config) > `--keystore` >
//! `--account` > the default wallet.

use std::path::PathBuf;

use hypersdk::Address;
use hypersdk::hypercore::PrivateKeySigner;
use ows_lib::types::WalletInfo;

use crate::errors::CliError;
use crate::legacy_accounts::{LegacyAccount, LegacyAccounts};
use crate::ows::signing::OwsSigningConfig;
use crate::ows::vault::{HyperliquidAccount, Vault};
use crate::settings::{Settings, env};
use crate::signing::SelectedSigner;
pub use crate::signing::SignerSource;

#[derive(Debug, Clone)]
pub struct ResolvedSigner {
    selected_signer: SelectedSigner,
}

impl ResolvedSigner {
    #[must_use]
    pub fn new(selected_signer: SelectedSigner) -> Self {
        Self { selected_signer }
    }

    #[must_use]
    pub fn selected_signer(&self) -> SelectedSigner {
        self.selected_signer.clone()
    }

    #[must_use]
    pub fn address(&self) -> Address {
        self.selected_signer.address()
    }

    #[must_use]
    pub fn query_address(&self) -> Address {
        self.selected_signer.query_address()
    }

    #[must_use]
    pub fn source(&self) -> &SignerSource {
        self.selected_signer.source()
    }

    #[must_use]
    pub fn ows_wallet_name(&self) -> Option<&str> {
        self.selected_signer.ows_wallet_name()
    }

    pub fn local_private_key(&self) -> Result<&PrivateKeySigner, CliError> {
        self.selected_signer.private_key_signer()
    }
}

/// The signer inputs of one run.
#[derive(Debug, Clone, Default)]
pub struct SignerSelection {
    pub ows: Option<String>,
    pub private_key: Option<String>,
    pub keystore: Option<PathBuf>,
    pub keystore_password: Option<String>,
    pub account: Option<String>,
}

impl SignerSelection {
    /// Gather the run's signer inputs.
    ///
    /// `--ows-signer` and `--account` name a wallet explicitly, so an ambient
    /// private key (`HYPERLIQUID_PRIVATE_KEY` or the legacy config key) must not
    /// override them: the key is not even read when either is set.
    pub fn from_flags(
        settings: &Settings,
        cli_private_key: Option<&str>,
        keystore: Option<PathBuf>,
        keystore_password: Option<String>,
        account: Option<String>,
        ows: Option<String>,
    ) -> Result<Self, CliError> {
        let private_key = if account.is_some() || ows.is_some() {
            None
        } else {
            settings.private_key(cli_private_key)?
        };
        Ok(Self {
            ows,
            private_key,
            keystore,
            keystore_password,
            account,
        })
    }

    /// Whether nothing names a signer, so the default wallet applies.
    #[must_use]
    pub fn picks_default(&self) -> bool {
        self.ows.is_none()
            && self.private_key.is_none()
            && self.keystore.is_none()
            && self.account.is_none()
    }

    /// Whether the signer is a vault wallet found by `--ows-signer` or by default,
    /// as opposed to one named through `--account`.
    #[must_use]
    pub fn names_wallet_directly(&self) -> bool {
        self.ows.is_some() || self.picks_default()
    }
}

pub fn parse_private_key(private_key: &str) -> Result<PrivateKeySigner, CliError> {
    private_key.parse::<PrivateKeySigner>().map_err(|_| {
        CliError::InvalidAuth("private key must be a 32-byte 0x-prefixed hex string".to_string())
    })
}

pub fn signer_private_key_hex(signer: &PrivateKeySigner) -> String {
    format!("0x{}", hex::encode(signer.to_bytes()))
}

pub fn resolve_signer(
    selection: &SignerSelection,
    settings: &Settings,
) -> Result<ResolvedSigner, anyhow::Error> {
    if let Some(selector) = selection.ows.as_deref() {
        return Ok(resolve_ows_flag(selector, settings)?);
    }
    if let Some(private_key) = selection.private_key.as_deref() {
        let signer = parse_private_key(private_key)?;
        let query_address = signer.address();
        return Ok(ResolvedSigner::new(SelectedSigner::local_private_key(
            signer,
            SignerSource::PrivateKey,
            query_address,
        )));
    }
    if let Some(path) = selection.keystore.as_deref() {
        let password = selection
            .keystore_password
            .clone()
            .or_else(|| settings.keystore_password())
            .ok_or_else(|| {
                CliError::InvalidAuth(format!(
                    "keystore password is required with --keystore; set {} or pass --keystore-password",
                    env::KEYSTORE_PASSWORD
                ))
            })?;
        let signer = PrivateKeySigner::decrypt_keystore(path, &password)
            .map_err(|err| CliError::InvalidAuth(format!("failed to decrypt keystore: {err}")))?;
        let query_address = signer.address();
        return Ok(ResolvedSigner::new(SelectedSigner::local_private_key(
            signer,
            SignerSource::Keystore,
            query_address,
        )));
    }
    if let Some(selector) = selection.account.as_deref() {
        return resolve_account(selector, settings);
    }
    resolve_default(settings)
}

fn ows_signer(
    selector: String,
    wallet: &WalletInfo,
    account: &HyperliquidAccount,
    vault: Vault,
) -> ResolvedSigner {
    ResolvedSigner::new(SelectedSigner::ows(OwsSigningConfig::for_wallet(
        selector,
        wallet,
        account.address,
        vault,
    )))
}

/// `--ows-signer`: a vault wallet by name or id, or a raw address that can only
/// preview. A raw address never matches a vault wallet here, unlike `--account`.
fn resolve_ows_flag(selector: &str, settings: &Settings) -> Result<ResolvedSigner, CliError> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(CliError::Configuration(
            "--ows-signer requires a non-empty selector".to_string(),
        ));
    }
    if let Ok(address) = selector.parse::<Address>() {
        return Ok(ResolvedSigner::new(SelectedSigner::ows(
            OwsSigningConfig::address_only(selector.to_string(), address),
        )));
    }
    let vault = Vault::from_settings(settings);
    let wallet = vault.get(selector)?;
    let account = Vault::hyperliquid_account(&wallet)?;
    Ok(ows_signer(selector.to_string(), &wallet, &account, vault))
}

/// A vault wallet named by `--account`: by name or id, or by its Hyperliquid
/// address (compared case-insensitively). `Ok(None)` for an address no wallet owns.
fn find_vault_account(
    vault: &Vault,
    selector: &str,
) -> Result<Option<(WalletInfo, HyperliquidAccount)>, CliError> {
    if selector.parse::<Address>().is_ok() {
        // A vault that can't be listed reads as "no wallet owns this address".
        for wallet in vault.list().unwrap_or_default() {
            if let Ok(account) = Vault::hyperliquid_account(&wallet)
                && account.address_text.eq_ignore_ascii_case(selector)
            {
                return Ok(Some((wallet, account)));
            }
        }
        return Ok(None);
    }
    let wallet = vault.get(selector)?;
    let account = Vault::hyperliquid_account(&wallet)?;
    Ok(Some((wallet, account)))
}

fn resolve_account(selector: &str, settings: &Settings) -> Result<ResolvedSigner, anyhow::Error> {
    let vault = Vault::from_settings(settings);
    match find_vault_account(&vault, selector) {
        Ok(Some((wallet, account))) => {
            return Ok(ows_signer(wallet.name.clone(), &wallet, &account, vault));
        }
        Ok(None) | Err(CliError::OwsWalletNotFound { .. } | CliError::OwsNoChainAccount { .. }) => {
        }
        Err(err) => return Err(err.into()),
    }

    let Some(store) = LegacyAccounts::open_default(settings)? else {
        return Err(account_selector_not_found(selector).into());
    };
    let account = store
        .account_by_selector(selector)?
        .ok_or_else(|| account_selector_not_found(selector))?;
    legacy_signer(&store, account)
}

fn resolve_default(settings: &Settings) -> Result<ResolvedSigner, anyhow::Error> {
    let vault = Vault::from_settings(settings);
    match vault.default_wallet(settings.default_wallet_id()?) {
        Ok(wallet) => {
            let account = Vault::hyperliquid_account(&wallet)?;
            return Ok(ows_signer(wallet.name.clone(), &wallet, &account, vault));
        }
        Err(
            CliError::OwsWalletNotFound { .. }
            | CliError::OwsNoChainAccount { .. }
            | CliError::AuthRequired,
        ) => {}
        Err(err) => return Err(err.into()),
    }

    let Some(store) = LegacyAccounts::open_default(settings)? else {
        return Err(CliError::AuthRequired.into());
    };
    let account = store
        .default_or_first_account()?
        .ok_or(CliError::AuthRequired)?;
    legacy_signer(&store, account)
}

fn legacy_signer(
    store: &LegacyAccounts,
    account: LegacyAccount,
) -> Result<ResolvedSigner, anyhow::Error> {
    let private_key = store.decrypt_private_key(&account)?;
    let signer = parse_private_key(&private_key)?;
    let query_address = account.query_address()?;
    Ok(ResolvedSigner::new(SelectedSigner::local_private_key(
        signer,
        SignerSource::StoredAccount {
            alias: account.alias,
        },
        query_address,
    )))
}

pub(crate) fn account_selector_not_found(selector: &str) -> CliError {
    CliError::Unsupported(format!(
        "account selector '{selector}' was not found as an address, alias, or id"
    ))
}

/// The address an account selector stands for in data lookups: the address itself,
/// a vault wallet's Hyperliquid address, or a legacy account's query address.
///
/// Unlike signing, a vault that errors or holds no matching wallet falls through
/// to the legacy store instead of failing.
pub(crate) fn lookup_account_address(
    selector: &str,
    settings: &Settings,
) -> Result<Address, anyhow::Error> {
    let trimmed = selector.trim();
    if let Ok(address) = trimmed.parse::<Address>() {
        return Ok(address);
    }

    if trimmed.starts_with("0x") {
        return Err(CliError::Unsupported(format!(
            "Invalid address: {trimmed}; expected a 0x-prefixed 40-hex-character (20-byte) string"
        ))
        .into());
    }

    let vault = Vault::from_settings(settings);
    if let Ok(wallet) = vault.get(trimmed)
        && let Ok(account) = Vault::hyperliquid_account(&wallet)
    {
        return Ok(account.address);
    }

    let Some(store) = LegacyAccounts::open_default(settings)? else {
        return Err(account_selector_not_found(trimmed).into());
    };
    let account = store
        .account_by_selector(trimmed)?
        .ok_or_else(|| account_selector_not_found(trimmed))?;
    Ok(account.query_address()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::ows::vault::test_support::import_test_wallet;
    use crate::settings::EnvSource;
    use tempfile::TempDir;

    const KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000009";
    const RAW_OWS_ADDRESS: &str = "0x0000000000000000000000000000000000000001";

    fn vault_settings(vault: &TempDir, config: Option<Config>) -> Settings {
        Settings::detached(
            EnvSource::fixed([(env::OWS_VAULT_PATH, vault.path().to_str().unwrap())]),
            config,
        )
    }

    fn selection() -> SignerSelection {
        SignerSelection::default()
    }

    #[test]
    fn parses_private_key_and_derives_address() {
        let signer = parse_private_key(KEY).unwrap();
        assert!(signer.address().to_string().starts_with("0x"));
        assert_eq!(signer_private_key_hex(&signer), KEY);
    }

    #[test]
    fn invalid_private_key_maps_to_auth_error() {
        assert_eq!(parse_private_key("not-a-key").unwrap_err().exit_code(), 10);
    }

    #[test]
    fn ows_flag_beats_every_other_source_and_raw_address_cannot_sign() {
        let vault = TempDir::new().unwrap();
        let resolved = resolve_signer(
            &SignerSelection {
                ows: Some(RAW_OWS_ADDRESS.to_string()),
                private_key: Some(KEY.to_string()),
                keystore: Some(PathBuf::from("/nonexistent")),
                account: Some("main".to_string()),
                ..selection()
            },
            &vault_settings(&vault, None),
        )
        .unwrap();

        assert_eq!(resolved.address().to_string(), RAW_OWS_ADDRESS);
        assert_eq!(resolved.query_address(), resolved.address());
        assert!(matches!(resolved.source(), SignerSource::Ows { .. }));
        assert!(resolved.local_private_key().is_err());
        assert_eq!(resolved.ows_wallet_name(), None);
    }

    #[test]
    fn ows_flag_does_not_match_a_vault_wallet_by_address_but_account_does() {
        let vault = TempDir::new().unwrap();
        let wallet = import_test_wallet(vault.path(), "main");
        let account = Vault::hyperliquid_account(&wallet).unwrap();
        let settings = vault_settings(&vault, None);

        let by_flag = resolve_signer(
            &SignerSelection {
                ows: Some(account.address_text.clone()),
                ..selection()
            },
            &settings,
        )
        .unwrap();
        assert!(
            by_flag
                .selected_signer()
                .ensure_can_attempt_live_signing()
                .is_err()
        );

        let by_account = resolve_signer(
            &SignerSelection {
                account: Some(account.address_text.to_lowercase()),
                ..selection()
            },
            &settings,
        )
        .unwrap();
        assert!(
            by_account
                .selected_signer()
                .ensure_can_attempt_live_signing()
                .is_ok()
        );
        assert_eq!(by_account.address(), account.address);
        assert_eq!(by_account.ows_wallet_name(), Some("main"));
        assert_eq!(
            by_account.source(),
            &SignerSource::Ows {
                selector: "main".to_string()
            }
        );
    }

    #[test]
    fn ows_flag_resolves_wallet_by_name_and_keeps_the_selector() {
        let vault = TempDir::new().unwrap();
        let wallet = import_test_wallet(vault.path(), "main");
        let resolved = resolve_signer(
            &SignerSelection {
                ows: Some(format!(" {} ", wallet.id)),
                ..selection()
            },
            &vault_settings(&vault, None),
        )
        .unwrap();

        assert_eq!(resolved.ows_wallet_name(), Some("main"));
        assert_eq!(
            resolved.source(),
            &SignerSource::Ows {
                selector: wallet.id.clone()
            }
        );
    }

    #[test]
    fn empty_ows_selector_is_a_configuration_error() {
        let vault = TempDir::new().unwrap();
        let err = resolve_signer(
            &SignerSelection {
                ows: Some("  ".to_string()),
                ..selection()
            },
            &vault_settings(&vault, None),
        )
        .unwrap_err();
        assert!(err.to_string().contains("--ows-signer requires"));
    }

    #[test]
    fn private_key_beats_keystore() {
        let vault = TempDir::new().unwrap();
        let resolved = resolve_signer(
            &SignerSelection {
                private_key: Some(KEY.to_string()),
                keystore: Some(PathBuf::from("/nonexistent")),
                ..selection()
            },
            &vault_settings(&vault, None),
        )
        .unwrap();
        assert_eq!(resolved.source(), &SignerSource::PrivateKey);
        assert_eq!(
            resolved.local_private_key().unwrap().address(),
            resolved.address()
        );
    }

    #[test]
    fn keystore_requires_a_password() {
        let vault = TempDir::new().unwrap();
        let err = resolve_signer(
            &SignerSelection {
                keystore: Some(PathBuf::from("/nonexistent")),
                ..selection()
            },
            &vault_settings(&vault, None),
        )
        .unwrap_err();
        assert!(err.to_string().contains(env::KEYSTORE_PASSWORD));
        assert_eq!(
            err.downcast_ref::<CliError>().map(CliError::exit_code),
            Some(10)
        );
    }

    #[test]
    fn default_is_the_configured_wallet_or_the_only_wallet() {
        let vault = TempDir::new().unwrap();
        let first = import_test_wallet(vault.path(), "first");

        let only = resolve_signer(&selection(), &vault_settings(&vault, None)).unwrap();
        assert_eq!(only.ows_wallet_name(), Some("first"));

        import_test_wallet(vault.path(), "second");
        let config = Config {
            default_wallet_id: Some(first.id.clone()),
            ..Config::default()
        };
        let configured =
            resolve_signer(&selection(), &vault_settings(&vault, Some(config))).unwrap();
        assert_eq!(configured.ows_wallet_name(), Some("first"));

        let err = resolve_signer(&selection(), &vault_settings(&vault, None)).unwrap_err();
        assert!(err.to_string().contains("holds 2 wallets"));
    }

    #[test]
    fn selection_ignores_ambient_key_when_a_wallet_is_named() {
        let settings = Settings::detached(EnvSource::fixed([(env::PRIVATE_KEY, KEY)]), None);
        let named = SignerSelection::from_flags(
            &settings,
            None,
            None,
            None,
            Some("main".to_string()),
            None,
        )
        .unwrap();
        assert_eq!(named.private_key, None);
        assert!(!named.picks_default());

        let ambient = SignerSelection::from_flags(&settings, None, None, None, None, None).unwrap();
        assert_eq!(ambient.private_key.as_deref(), Some(KEY));
        assert!(!ambient.names_wallet_directly());

        let bare = SignerSelection::from_flags(
            &Settings::detached(EnvSource::fixed::<&str, &str>([]), None),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(bare.picks_default());
        assert!(bare.names_wallet_directly());
    }
}
