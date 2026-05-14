//! Authentication and signer resolution.

use std::path::Path;

use hypersdk::Address;
use hypersdk::hypercore::PrivateKeySigner;

use crate::db::AccountStore;
use crate::errors::CliError;
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

    pub fn local_private_key(&self) -> Result<&PrivateKeySigner, CliError> {
        self.selected_signer.private_key_signer()
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
    resolved_private_key: Option<&str>,
    keystore_path: Option<&Path>,
    keystore_password: Option<&str>,
) -> Result<ResolvedSigner, anyhow::Error> {
    resolve_signer_with_account(resolved_private_key, keystore_path, keystore_password, None)
}

pub fn resolve_signer_with_account(
    resolved_private_key: Option<&str>,
    keystore_path: Option<&Path>,
    keystore_password: Option<&str>,
    account_selector: Option<&str>,
) -> Result<ResolvedSigner, anyhow::Error> {
    resolve_signer_with_account_and_ows(
        resolved_private_key,
        keystore_path,
        keystore_password,
        account_selector,
        None,
    )
}

pub fn resolve_signer_with_account_and_ows(
    resolved_private_key: Option<&str>,
    keystore_path: Option<&Path>,
    keystore_password: Option<&str>,
    account_selector: Option<&str>,
    ows_selector: Option<&str>,
) -> Result<ResolvedSigner, anyhow::Error> {
    if let Some(selector) = ows_selector {
        let ows = crate::ows::resolve_selector(selector)?;
        let signing_config = crate::ows::OwsSigningConfig::with_vault_path(
            ows.selector().to_string(),
            ows.wallet().map(|wallet| wallet.wallet_id().to_string()),
            ows.address(),
            ows.vault_path().cloned(),
        );
        return Ok(ResolvedSigner::new(SelectedSigner::ows(signing_config)));
    }

    if let Some(private_key) = resolved_private_key {
        let signer = parse_private_key(private_key)?;
        let query_address = signer.address();
        return Ok(ResolvedSigner::new(SelectedSigner::local_private_key(
            signer,
            SignerSource::PrivateKey,
            query_address,
        )));
    }

    if let Some(path) = keystore_path {
        let password = keystore_password.ok_or_else(|| {
            CliError::InvalidAuth("keystore password is required with --keystore".to_string())
        })?;
        return Ok(resolve_keystore_signer(path, password)?);
    }

    if let Some(selector) = account_selector {
        return resolve_stored_account_signer(selector);
    }

    resolve_stored_default_signer()
}

pub fn resolve_stored_account_signer(selector: &str) -> Result<ResolvedSigner, anyhow::Error> {
    // Try OWS first: selector may be an OWS wallet name or id.
    let vault_path = crate::ows::ows_vault_path();
    match crate::ows::resolve_ows_wallet_selector(selector, vault_path.as_deref()) {
        Ok(crate::ows::ResolvedOwsSelector::Wallet {
            wallet, address, ..
        }) => {
            let signing_config = crate::ows::OwsSigningConfig::with_vault_path(
                wallet.name.clone(),
                Some(wallet.id.clone()),
                address,
                vault_path.clone(),
            );
            return Ok(ResolvedSigner::new(SelectedSigner::ows(signing_config)));
        }
        Ok(crate::ows::ResolvedOwsSelector::RawAddress { .. }) => {
            // Raw addresses can't sign — fall through to legacy store.
        }
        Err(ref e) if matches!(e, CliError::OwsWalletNotFound { .. }) => {
            // Wallet not found in OWS vault — fall through to legacy store.
        }
        Err(ref e) if matches!(e, CliError::OwsNoChainAccount { .. }) => {
            // No Hyperliquid account — fall through to legacy store.
        }
        Err(e) => return Err(e.into()),
    }

    // Fallback: legacy SQLite account store.
    let Some(store) = AccountStore::open_existing_default()? else {
        return Err(account_selector_not_found(selector).into());
    };
    let account = store
        .account_by_selector(selector)?
        .ok_or_else(|| account_selector_not_found(selector))?;
    let private_key = store.decrypt_account_private_key(&account)?;
    let signer = parse_private_key(&private_key)?;
    let query_address = account_query_address(&account)?;
    Ok(ResolvedSigner::new(SelectedSigner::local_private_key(
        signer,
        SignerSource::StoredAccount {
            alias: account.alias,
        },
        query_address,
    )))
}

pub fn resolve_stored_default_signer() -> Result<ResolvedSigner, anyhow::Error> {
    // Try OWS default wallet first.
    let vault_path = crate::ows::ows_vault_path();
    let config = crate::config::load_config().ok().flatten();
    let default_wallet_id = config.as_ref().and_then(|c| c.default_wallet_id.as_deref());
    match crate::ows::resolve_default_ows_wallet(default_wallet_id, vault_path.as_deref()) {
        Ok(wallet) => {
            let (_address_str, address) = crate::ows::hyperliquid_address_from_wallet(&wallet)
                .map_err(|err| CliError::InvalidAuth(err.to_string()))?;
            let signing_config = crate::ows::OwsSigningConfig::with_vault_path(
                wallet.name.clone(),
                Some(wallet.id.clone()),
                address,
                vault_path.clone(),
            );
            return Ok(ResolvedSigner::new(SelectedSigner::ows(signing_config)));
        }
        Err(ref e)
            if matches!(e, CliError::OwsWalletNotFound { .. })
                || matches!(e, CliError::OwsNoChainAccount { .. })
                || matches!(e, CliError::AuthRequired) =>
        {
            // No matching OWS wallet — fall through to legacy store.
        }
        Err(e) => return Err(e.into()),
    }

    // Fallback: legacy SQLite account store.
    let Some(store) = AccountStore::open_existing_default()? else {
        return Err(CliError::AuthRequired.into());
    };
    let account = store
        .default_or_first_account()?
        .ok_or(CliError::AuthRequired)?;
    let private_key = store.decrypt_account_private_key(&account)?;
    let signer = parse_private_key(&private_key)?;
    let query_address = account_query_address(&account)?;
    Ok(ResolvedSigner::new(SelectedSigner::local_private_key(
        signer,
        SignerSource::StoredAccount {
            alias: account.alias,
        },
        query_address,
    )))
}

pub fn account_selector_not_found(selector: &str) -> CliError {
    CliError::Unsupported(format!(
        "account selector '{selector}' was not found as an address, alias, or id"
    ))
}

pub fn resolve_keystore_signer(
    keystore_path: &Path,
    password: &str,
) -> Result<ResolvedSigner, CliError> {
    let signer = PrivateKeySigner::decrypt_keystore(keystore_path, password)
        .map_err(|err| CliError::InvalidAuth(format!("failed to decrypt keystore: {err}")))?;
    let query_address = signer.address();
    Ok(ResolvedSigner::new(SelectedSigner::local_private_key(
        signer,
        SignerSource::Keystore,
        query_address,
    )))
}

fn account_query_address(account: &crate::db::Account) -> Result<Address, CliError> {
    let raw = account
        .master_address
        .as_deref()
        .unwrap_or(account.address.as_str());
    raw.parse::<Address>().map_err(|_| {
        CliError::Internal(anyhow::anyhow!(
            "stored account '{}' has invalid query address {}",
            account.alias,
            raw
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::AccountStore;
    use tempfile::TempDir;

    const KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000009";

    #[test]
    fn parses_private_key_and_derives_address() {
        let signer = parse_private_key(KEY).unwrap();
        assert!(signer.address().to_string().starts_with("0x"));
    }

    #[test]
    fn invalid_private_key_maps_to_auth_error() {
        let err = parse_private_key("not-a-key").unwrap_err();
        assert_eq!(err.exit_code(), 10);
    }

    #[test]
    fn signer_private_key_hex_roundtrips() {
        let signer = parse_private_key(KEY).unwrap();
        assert_eq!(signer_private_key_hex(&signer), KEY);
    }

    #[test]
    fn stored_default_account_can_be_decrypted_and_parsed() {
        let tmp = TempDir::new().unwrap();
        let mut store =
            AccountStore::open(tmp.path().join("accounts.db"), tmp.path().join("key")).unwrap();
        let signer = parse_private_key(KEY).unwrap();
        let account = store
            .add_account(
                "main",
                &signer.address().to_string(),
                KEY,
                "api-wallet",
                true,
            )
            .unwrap();
        let decrypted = store.decrypt_account_private_key(&account).unwrap();
        assert_eq!(
            parse_private_key(&decrypted).unwrap().address(),
            signer.address()
        );
    }

    #[test]
    fn resolved_signer_converts_to_selected_signer_without_changing_identity() {
        let signer = parse_private_key(KEY).unwrap();
        let address = signer.address();
        let resolved = ResolvedSigner::new(SelectedSigner::local_private_key(
            signer,
            SignerSource::StoredAccount {
                alias: "main".to_string(),
            },
            address,
        ));

        let selected = resolved.selected_signer();

        assert_eq!(selected.address(), address);
        assert_eq!(selected.query_address(), address);
        assert_eq!(
            selected.source(),
            &crate::signing::SignerSource::StoredAccount {
                alias: "main".to_string(),
            }
        );
    }

    #[test]
    fn resolves_experimental_ows_signer() {
        let resolved = resolve_signer_with_account_and_ows(
            None,
            None,
            None,
            None,
            Some("0x0000000000000000000000000000000000000001"),
        )
        .unwrap();

        assert_eq!(
            resolved.address().to_string(),
            "0x0000000000000000000000000000000000000001"
        );
        assert_eq!(resolved.query_address(), resolved.address());
        assert!(matches!(resolved.source(), SignerSource::Ows { .. }));
    }
}
