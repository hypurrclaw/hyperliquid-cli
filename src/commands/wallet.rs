//! Wallet and local account management commands.
//!
//! OWS (Open Wallet Standard) is the primary and only wallet backend.
//! All wallet creation, import, listing, and signing flow through the OWS vault
//! at ~/.hyperliquid (or HYPERLIQUID_OWS_VAULT_PATH when set).

use std::path::Path;

use serde::Serialize;

use crate::auth::{self, SignerSource};
use crate::config::{self, Config};
use crate::errors::CliError;
use crate::output::{self, OutputFormat, TableData};
use crate::prompt;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AccountRow {
    pub id: i64,
    pub alias: String,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    #[serde(rename = "type")]
    pub account_type: String,
    pub is_default: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountsOutput {
    accounts: Vec<AccountRow>,
}

impl AccountsOutput {
    #[must_use]
    pub fn new(accounts: Vec<AccountRow>) -> Self {
        Self { accounts }
    }
}

impl TableData for AccountsOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "ID",
            "Alias",
            "Address",
            "Master",
            "Agent Name",
            "Expires At",
            "Type",
            "Default",
            "Created At",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.accounts
            .iter()
            .map(|account| {
                vec![
                    account.id.to_string(),
                    account.alias.clone(),
                    account.address.clone(),
                    account
                        .master_address
                        .clone()
                        .unwrap_or_else(|| "n/a".to_string()),
                    account
                        .agent_name
                        .clone()
                        .unwrap_or_else(|| "n/a".to_string()),
                    account
                        .expires_at
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "n/a".to_string()),
                    account_type_display_label(&account.account_type).to_string(),
                    if account.is_default { "yes" } else { "no" }.to_string(),
                    account.created_at.clone(),
                ]
            })
            .collect()
    }

    fn empty_message(&self) -> Option<&str> {
        Some("No accounts.")
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.accounts).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct WalletInfo {
    message: String,
    address: String,
    alias: Option<String>,
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_default: Option<bool>,
    config_path: String,
    vault_path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    config_keys_written: Vec<String>,
}

impl TableData for WalletInfo {
    fn headers(&self) -> Vec<&str> {
        vec!["Field", "Value"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        let mut rows = vec![
            vec!["Message".to_string(), self.message.clone()],
            vec!["Address".to_string(), self.address.clone()],
            vec![
                "Alias".to_string(),
                self.alias.clone().unwrap_or_else(|| "n/a".to_string()),
            ],
            vec!["Source".to_string(), self.source.clone()],
            vec![
                "Default".to_string(),
                self.is_default
                    .map(|is_default| if is_default { "yes" } else { "no" }.to_string())
                    .unwrap_or_else(|| "n/a".to_string()),
            ],
            vec!["Config".to_string(), self.config_path.clone()],
            vec!["Vault".to_string(), self.vault_path.clone()],
        ];
        if !self.config_keys_written.is_empty() {
            rows.push(vec![
                "Config keys written".to_string(),
                self.config_keys_written.join(", "),
            ]);
        }
        rows
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct WalletAddressOutput {
    address: String,
}

impl TableData for WalletAddressOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Address"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![self.address.clone()]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct StatusOutput {
    status: String,
    message: String,
}

impl TableData for StatusOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Status", "Message"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![self.status.clone(), self.message.clone()]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// A wallet just stored in the vault and made the default in config.
struct NewWallet {
    wallet: ows_lib::types::WalletInfo,
    address: String,
    config_keys_written: Vec<String>,
}

type CreateWallet<'a> = Box<
    dyn FnOnce(Option<&str>, Option<&Path>) -> Result<ows_lib::types::WalletInfo, anyhow::Error>
        + 'a,
>;

/// Create a vault wallet and make it the default, under the config lock.
///
/// The config is loaded first so a broken file fails before a wallet exists,
/// and the wallet is removed again if the config can't be saved.
fn store_new_wallet(create: CreateWallet<'_>) -> Result<NewWallet, anyhow::Error> {
    crate::ows::warn_if_vault_passphrase_unset();
    let passphrase = crate::ows::ows_passphrase();
    let vault_path = crate::ows::ows_vault_path();

    let _lock = config::lock_config()?;
    let mut cfg = config::load_config()?.unwrap_or_default();
    let mut config_keys_written = apply_setup_defaults_to_config(&mut cfg);

    let wallet = create(passphrase.as_deref(), vault_path.as_deref())?;
    let (address, _) = crate::ows::hyperliquid_address_from_wallet(&wallet)
        .map_err(|err| anyhow::anyhow!("{err}"))?;

    // Mark the new wallet as default in config.
    if let Some(prev) = cfg.default_wallet_id.as_deref()
        && prev != wallet.id
    {
        eprintln!("! Default wallet changed from '{prev}' to '{}'.", wallet.id);
    }
    cfg.default_wallet_id = Some(wallet.id.clone());
    config_keys_written.insert(0, "default_wallet_id".to_string());
    config::save_config(&cfg).map_err(|err| {
        let _ = crate::ows::delete_ows_wallet(&wallet.id, vault_path.as_deref());
        anyhow::anyhow!("failed to save config: {err}")
    })?;

    Ok(NewWallet {
        wallet,
        address,
        config_keys_written,
    })
}

fn print_new_wallet(message: &str, new: NewWallet, format: OutputFormat) {
    print_wallet_receipt(
        message,
        &new.address,
        Some(new.wallet.name),
        "stored OWS wallet",
        Some(true),
        new.config_keys_written,
        format,
    );
}

pub fn create(format: OutputFormat) -> Result<(), anyhow::Error> {
    let new = store_new_wallet(Box::new(|passphrase, vault_path| {
        let name = next_ows_name("wallet", vault_path)?;
        crate::ows::create_ows_wallet(&name, passphrase, vault_path)
            .map_err(|err| anyhow::anyhow!("{err}"))
    }))?;
    print_new_wallet("Created wallet", new, format);
    Ok(())
}

/// Apply packaged builder/referral defaults when the config does not already define them.
///
/// Returns the config keys it wrote.
fn apply_setup_defaults_to_config(cfg: &mut Config) -> Vec<String> {
    let mut written = Vec::new();
    let has_builder_address = cfg
        .default_builder_address
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let has_builder_fee = cfg
        .default_builder_fee_rate
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if !has_builder_address
        && !has_builder_fee
        && let Ok(Some((address, fee))) = crate::commands::setup::setup_builder_default_suggestion()
    {
        cfg.default_builder_address = Some(address);
        cfg.default_builder_fee_rate = Some(fee);
        written.push("default_builder_address".to_string());
        written.push("default_builder_fee_rate".to_string());
    }

    let has_referral_code = cfg
        .default_referral_code
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if !has_referral_code
        && let Ok(Some(code)) =
            crate::commands::setup::setup_referral_default_suggestion(cfg.network)
    {
        cfg.default_referral_code = Some(code);
        written.push("default_referral_code".to_string());
    }
    written
}

/// Find the next available wallet name by appending "-2", "-3", etc.
fn next_ows_name(
    base: &str,
    vault_path: Option<&std::path::Path>,
) -> Result<String, anyhow::Error> {
    let existing =
        crate::ows::list_ows_wallets(vault_path).map_err(|err| anyhow::anyhow!("{err}"))?;
    let names: Vec<&str> = existing.iter().map(|w| w.name.as_str()).collect();
    if !names.contains(&base) {
        return Ok(base.to_string());
    }
    for i in 2.. {
        let candidate = format!("{base}-{i}");
        if !names.contains(&candidate.as_str()) {
            return Ok(candidate);
        }
    }
    unreachable!()
}

pub fn import(private_key: Option<&str>, format: OutputFormat) -> Result<(), anyhow::Error> {
    let private_key = match private_key {
        Some(private_key) => {
            output::warn(
                "passing a private key as an argument can expose it in OS process listings and shell history. Prefer `hyperliquid wallet import` and paste at the hidden prompt.",
            );
            private_key.trim().to_string()
        }
        None => prompt_private_key()?.trim().to_string(),
    };

    let new = store_new_wallet(Box::new(|passphrase, vault_path| {
        let name = next_ows_name("imported", vault_path)?;
        crate::ows::import_ows_wallet_private_key(&name, &private_key, passphrase, vault_path)
            .map_err(|err| anyhow::anyhow!("{err}"))
    }))?;
    print_new_wallet("Imported wallet", new, format);
    Ok(())
}

pub fn import_mnemonic(
    mnemonic: Option<&str>,
    alias: Option<&str>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let mnemonic = match mnemonic {
        Some(mnemonic) => {
            output::warn(
                "passing a mnemonic as an argument can expose it in OS process listings and shell history. Prefer `hyperliquid wallet import-mnemonic` and paste at the hidden prompt.",
            );
            mnemonic.trim().to_string()
        }
        None => prompt::read_secret("Mnemonic phrase: ", "a mnemonic phrase on stdin")?
            .trim()
            .to_string(),
    };

    let new = store_new_wallet(Box::new(|passphrase, vault_path| {
        let name = match alias {
            Some(alias) => alias.trim().to_string(),
            None => next_ows_name("imported", vault_path)?,
        };
        crate::ows::import_ows_wallet_mnemonic(&name, &mnemonic, passphrase, vault_path)
            .map_err(|err| anyhow::anyhow!("{err}"))
    }))?;
    print_new_wallet("Imported wallet", new, format);
    Ok(())
}

pub fn show(
    resolved_private_key: Option<&str>,
    keystore_path: Option<&Path>,
    keystore_password: Option<&str>,
    account_selector: Option<&str>,
    ows_selector: Option<&str>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    // When --ows-signer is explicit, resolve the specific wallet.
    // When no signer at all is specified, auto-detect the first OWS wallet.
    let vault_path = crate::ows::ows_vault_path();
    if ows_selector.is_some()
        || (resolved_private_key.is_none() && keystore_path.is_none() && account_selector.is_none())
    {
        let wallet = if let Some(selector) = ows_selector {
            // Resolve explicit --ows-signer selector (wallet name or id)
            match crate::ows::get_ows_wallet(selector, vault_path.as_deref()) {
                Ok(w) => w,
                Err(CliError::InvalidAuth(_) | CliError::OwsWalletNotFound { .. }) => {
                    // Not a known OWS wallet — fall through to traditional resolver
                    // for 0x addresses, stored accounts, etc.
                    let resolved = auth::resolve_signer_with_account_and_ows(
                        resolved_private_key,
                        keystore_path,
                        keystore_password,
                        account_selector,
                        Some(selector),
                    )?;
                    let (alias, source) = signer_display(resolved.source());
                    print_wallet_info(
                        "Current wallet",
                        &resolved.address().to_string(),
                        alias,
                        &source,
                        None,
                        format,
                    );
                    return Ok(());
                }
                Err(e) => return Err(anyhow::anyhow!("{e}")),
            }
        } else {
            // Auto-detect: prefer configured default wallet, then first compatible wallet.
            let cfg = config::load_config()?;
            let default_wallet_id = cfg.as_ref().and_then(|c| c.default_wallet_id.as_deref());
            match crate::ows::resolve_default_ows_wallet(default_wallet_id, vault_path.as_deref()) {
                Ok(wallet) => wallet,
                _ => {
                    // Fall through to traditional resolver
                    let resolved = auth::resolve_signer_with_account_and_ows(
                        resolved_private_key,
                        keystore_path,
                        keystore_password,
                        account_selector,
                        ows_selector,
                    )?;
                    let (alias, source) = signer_display(resolved.source());
                    print_wallet_info(
                        "Current wallet",
                        &resolved.address().to_string(),
                        alias,
                        &source,
                        None,
                        format,
                    );
                    return Ok(());
                }
            }
        };

        let (address_str, _address) = crate::ows::hyperliquid_address_from_wallet(&wallet)
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let source = format!("OWS wallet ({})", wallet.name);
        print_wallet_info(
            "Current wallet",
            &address_str,
            Some(wallet.name.clone()),
            &source,
            None,
            format,
        );
        return Ok(());
    }

    let resolved = auth::resolve_signer_with_account_and_ows(
        resolved_private_key,
        keystore_path,
        keystore_password,
        account_selector,
        ows_selector,
    )?;
    let (alias, source) = signer_display(resolved.source());
    print_wallet_info(
        "Current wallet",
        &resolved.address().to_string(),
        alias,
        &source,
        None,
        format,
    );
    Ok(())
}

fn signer_display(source: &SignerSource) -> (Option<String>, String) {
    match source {
        SignerSource::PrivateKey => (None, "private key/config/env".to_string()),
        SignerSource::Keystore => (None, "keystore".to_string()),
        SignerSource::StoredAccount { alias } => {
            (Some(alias.clone()), "stored signing account".to_string())
        }
        SignerSource::Ows { selector } => (None, format!("OWS signer ({selector})")),
    }
}

pub fn address(
    resolved_private_key: Option<&str>,
    keystore_path: Option<&Path>,
    keystore_password: Option<&str>,
    account_selector: Option<&str>,
    ows_selector: Option<&str>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let vault_path = crate::ows::ows_vault_path();

    let address_str = if let Some(selector) = ows_selector {
        // Explicit --ows-signer: resolve the specific wallet
        match crate::ows::get_ows_wallet(selector, vault_path.as_deref()) {
            Ok(wallet) => {
                let (addr, _) = crate::ows::hyperliquid_address_from_wallet(&wallet)
                    .map_err(|err| anyhow::anyhow!("{err}"))?;
                addr
            }
            Err(CliError::InvalidAuth(_) | CliError::OwsWalletNotFound { .. }) => {
                // Not a known OWS wallet — fall back to traditional resolver
                // for 0x addresses, stored accounts, etc.
                resolve_fallback_address(
                    resolved_private_key,
                    keystore_path,
                    keystore_password,
                    account_selector,
                    ows_selector,
                )?
            }
            Err(e) => return Err(anyhow::anyhow!("{e}")),
        }
    } else if resolved_private_key.is_none()
        && keystore_path.is_none()
        && account_selector.is_none()
    {
        // Auto-detect: prefer configured default wallet, then first compatible wallet.
        let cfg = config::load_config()?;
        let default_wallet_id = cfg.as_ref().and_then(|c| c.default_wallet_id.as_deref());
        match crate::ows::resolve_default_ows_wallet(default_wallet_id, vault_path.as_deref()) {
            Ok(wallet) => crate::ows::hyperliquid_address_from_wallet(&wallet)
                .map(|(addr, _)| addr)
                .map_err(|err| anyhow::anyhow!("{err}"))?,
            Err(_) => resolve_fallback_address(
                resolved_private_key,
                keystore_path,
                keystore_password,
                account_selector,
                ows_selector,
            )?,
        }
    } else {
        resolve_fallback_address(
            resolved_private_key,
            keystore_path,
            keystore_password,
            account_selector,
            ows_selector,
        )?
    };

    if format == OutputFormat::Json {
        crate::output::print_data_no_timing(
            &WalletAddressOutput {
                address: address_str.clone(),
            },
            format,
        );
    } else {
        println!("{address_str}");
    }
    Ok(())
}

fn resolve_fallback_address(
    resolved_private_key: Option<&str>,
    keystore_path: Option<&Path>,
    keystore_password: Option<&str>,
    account_selector: Option<&str>,
    ows_selector: Option<&str>,
) -> Result<String, anyhow::Error> {
    let resolved = auth::resolve_signer_with_account_and_ows(
        resolved_private_key,
        keystore_path,
        keystore_password,
        account_selector,
        ows_selector,
    )?;
    Ok(resolved.address().to_string())
}

pub fn reset(yes: bool, format: OutputFormat) -> Result<(), anyhow::Error> {
    let Some(path) = config::config_file_path().filter(|path| path.exists()) else {
        print_status("noop", "Nothing to reset", format);
        return Ok(());
    };

    if !yes {
        require_promptable("--yes", format)?;
        eprintln!("This deletes the config file at {}.", path.display());
        eprintln!(
            "It clears the default wallet, network, builder, and referral defaults. Wallets stay in the vault."
        );
        if config::load_config_from_path(&path).is_err() {
            eprintln!("The file isn't valid config, so its contents are discarded.");
        }
        let question = format!("Delete config file {}?", path.display());
        if !prompt::confirm(&question, "--yes")? {
            print_declined("Reset cancelled", format);
            return Ok(());
        }
    }

    let _lock = config::lock_config()?;
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(CliError::Configuration(format!(
                "failed to remove config {}: {err}",
                path.display()
            ))
            .into());
        }
    }
    print_status(
        "reset",
        &format!("Deleted config file {}", path.display()),
        format,
    );
    Ok(())
}

pub fn account_add(
    private_key: Option<&str>,
    alias: Option<&str>,
    _account_type: Option<&str>,
    make_default: bool,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let automation_mode = private_key.is_some() || alias.is_some() || make_default;
    let private_key = match private_key {
        Some(private_key) => {
            output::warn(
                "passing a private key as an argument can expose it in OS process listings and shell history.",
            );
            private_key.trim().to_string()
        }
        None => prompt_private_key()?.trim().to_string(),
    };
    let name = match alias {
        Some(alias) => alias.trim().to_string(),
        None if automation_mode => {
            let vp = crate::ows::ows_vault_path();
            next_ows_name("account", vp.as_deref())?
        }
        None => {
            require_promptable("--alias", format)?;
            prompt::read_line("Wallet name: ", "--alias")?
        }
    };
    let name = if name.trim().is_empty() {
        let vp = crate::ows::ows_vault_path();
        next_ows_name("account", vp.as_deref())?
    } else {
        name.trim().to_string()
    };

    crate::ows::warn_if_vault_passphrase_unset();
    let passphrase = crate::ows::ows_passphrase();
    let vault_path = crate::ows::ows_vault_path();
    let _lock = if make_default {
        Some(config::lock_config()?)
    } else {
        None
    };
    let mut cfg = if make_default {
        // Load before creating the wallet so a broken config doesn't leave an orphan.
        Some(config::load_config()?.unwrap_or_default())
    } else {
        None
    };
    let wallet = crate::ows::import_ows_wallet_private_key(
        &name,
        &private_key,
        passphrase.as_deref(),
        vault_path.as_deref(),
    )
    .map_err(|err| anyhow::anyhow!("{err}"))?;

    if let Some(cfg) = cfg.as_mut() {
        cfg.default_wallet_id = Some(wallet.id.clone());
        config::save_config(cfg).map_err(|err| {
            let _ = crate::ows::delete_ows_wallet(&wallet.id, vault_path.as_deref());
            anyhow::anyhow!("failed to save config: {err}")
        })?;
    }

    print_ows_wallet_change("Account added", &wallet, format)?;
    Ok(())
}

pub fn account_ls(format: OutputFormat) -> Result<(), anyhow::Error> {
    let mut all_rows: Vec<AccountRow> = Vec::new();

    // OWS is the only wallet backend — propagate vault errors, don't silently fall through.
    let vault_path = crate::ows::ows_vault_path();
    let ows_wallets =
        crate::ows::list_ows_wallets(vault_path.as_deref()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let config = config::load_config()?;
    let default_id = config.as_ref().and_then(|c| c.default_wallet_id.as_deref());
    let mut display_index = 0usize;
    for w in ows_wallets.into_iter() {
        let hl_addr = match crate::ows::hyperliquid_address_from_wallet(&w) {
            Ok((addr, _)) => addr,
            // Only skip wallets that genuinely lack a Hyperliquid/EVM account;
            // propagate real errors (malformed addresses, vault corruption).
            Err(CliError::OwsNoChainAccount { .. }) => continue,
            Err(e) => return Err(anyhow::anyhow!("{e}")),
        };
        let is_default = default_id.is_some_and(|d| d == w.id || d == w.name);
        all_rows.push(AccountRow {
            id: (display_index + 1) as i64,
            alias: w.name,
            address: hl_addr,
            master_address: None,
            agent_name: None,
            expires_at: None,
            account_type: "ows-wallet".to_string(),
            is_default,
            created_at: w.created_at,
        });
        display_index += 1;
    }
    // With no configured default, signing uses the only wallet in the vault.
    if default_id.is_none() && all_rows.len() == 1 {
        all_rows[0].is_default = true;
    }

    crate::output::print_data_no_timing(&AccountsOutput::new(all_rows), format);
    Ok(())
}

pub fn account_set_default(
    selector: Option<&str>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    if selector.is_none() {
        require_promptable("<WALLET>", format)?;
    }
    let vault_path = crate::ows::ows_vault_path();
    let wallets = crate::ows::list_ows_wallets(vault_path.as_deref())
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    if wallets.is_empty() {
        return Err(no_stored_accounts_error().into());
    }
    let selector = match selector {
        Some(selector) => selector.trim().to_string(),
        None => {
            print_ows_wallet_choices(&wallets)?;
            prompt::read_line("Select wallet by name or id: ", "<WALLET>")?
                .trim()
                .to_string()
        }
    };
    let wallet = wallets
        .into_iter()
        .find(|w| w.name == selector || w.id == selector)
        .ok_or_else(|| wallet_not_found(&selector))?;

    let _lock = config::lock_config()?;
    let mut config = config::load_config()?.unwrap_or_default();
    config.default_wallet_id = Some(wallet.id.clone());
    config::save_config(&config)?;

    print_ows_wallet_change("Default account set", &wallet, format)?;
    Ok(())
}

pub fn account_remove(
    selector: Option<&str>,
    yes: bool,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    if selector.is_none() {
        require_promptable("<WALLET>", format)?;
    }
    let vault_path = crate::ows::ows_vault_path();
    let wallets = crate::ows::list_ows_wallets(vault_path.as_deref())
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    if wallets.is_empty() {
        return Err(no_stored_accounts_error().into());
    }
    let selector = match selector {
        Some(selector) => selector.trim().to_string(),
        None => {
            print_ows_wallet_choices(&wallets)?;
            prompt::read_line("Select wallet by name or id: ", "<WALLET>")?
                .trim()
                .to_string()
        }
    };
    let wallet = wallets
        .into_iter()
        .find(|w| w.name == selector || w.id == selector)
        .ok_or_else(|| wallet_not_found(&selector))?;
    let address = crate::ows::hyperliquid_address_from_wallet(&wallet)
        .map(|(address, _)| address)
        .unwrap_or_else(|_| "no Hyperliquid account".to_string());
    let is_default = config::load_config()?
        .and_then(|config| config.default_wallet_id)
        .is_some_and(|default| default == wallet.id || default == wallet.name);

    if !yes {
        require_promptable("--yes", format)?;
        eprintln!("Wallet:  {}", wallet.name);
        eprintln!("Address: {address}");
        eprintln!("Default: {}", if is_default { "yes" } else { "no" });
        eprintln!();
        eprintln!("This deletes the encrypted key from the vault. This cannot be undone.");
        eprintln!("Back up first:");
        eprintln!("    hyperliquid wallet export {}", shell_arg(&wallet.name));
        eprintln!();
        let confirmed = if output::terminal::stdin_is_terminal() {
            prompt::confirm_typed(
                &format!("To confirm, type the wallet name \"{}\": ", wallet.name),
                &wallet.name,
                "--yes",
            )?
        } else {
            prompt::confirm(&format!("Delete wallet {}?", wallet.name), "--yes")?
        };
        if !confirmed {
            print_declined("Delete cancelled", format);
            return Ok(());
        }
    }
    crate::ows::delete_ows_wallet(&wallet.id, vault_path.as_deref())
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    if is_default {
        // The deleted wallet was the default; clear the dangling reference.
        let _lock = config::lock_config()?;
        if let Some(mut config) = config::load_config()? {
            config.default_wallet_id = None;
            config::save_config(&config)?;
        }
        eprintln!("! The deleted wallet was the default. Pick another:");
        eprintln!("    hyperliquid account set-default <WALLET>");
    }
    print_wallet_info(
        "Deleted wallet",
        &address,
        Some(wallet.name.clone()),
        "OWS wallet",
        None,
        format,
    );
    Ok(())
}

/// List all wallets in the OWS vault.
pub fn list(format: OutputFormat) -> Result<(), anyhow::Error> {
    account_ls(format)
}

/// Delete an OWS wallet by name or id.
pub fn delete(selector: &str, yes: bool, format: OutputFormat) -> Result<(), anyhow::Error> {
    account_remove(Some(selector), yes, format)
}

/// Rename an OWS wallet.
pub fn rename(selector: &str, new_name: &str, format: OutputFormat) -> Result<(), anyhow::Error> {
    let vault_path = crate::ows::ows_vault_path();
    crate::ows::rename_ows_wallet(selector, new_name, vault_path.as_deref())
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    let wallet = crate::ows::get_ows_wallet(new_name, vault_path.as_deref())
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    print_ows_wallet_change("Wallet renamed", &wallet, format)?;
    Ok(())
}

/// Export an OWS wallet's secret.
///
/// NOTE: This command intentionally outputs plaintext private keys or mnemonics
/// to stdout, which is an exception to the AGENTS.md rule "Never log, print,
/// commit, or store plaintext private keys." The export is gated behind an
/// explicit interactive confirmation prompt (or --yes flag) and a warning
/// banner. The command's sole purpose is secret recovery/export.
pub fn export(selector: &str, yes: bool, format: OutputFormat) -> Result<(), anyhow::Error> {
    if !yes {
        require_promptable("--yes", format)?;
        let question = format!(
            "Export secret for wallet '{selector}'? This will display the private key or mnemonic."
        );
        if !prompt::confirm(&question, "--yes")? {
            print_declined("Export cancelled", format);
            return Ok(());
        }
    }

    let passphrase = crate::ows::ows_passphrase();
    let vault_path = crate::ows::ows_vault_path();
    let secret =
        crate::ows::export_ows_wallet(selector, passphrase.as_deref(), vault_path.as_deref())
            .map_err(|err| anyhow::anyhow!("{err}"))?;

    output::warn(
        "this output contains a plaintext private key or mnemonic. Do not share or log it.",
    );

    if format == OutputFormat::Json {
        let output = serde_json::json!({"secret": secret});
        println!(
            "{}",
            serde_json::to_string_pretty(&output).unwrap_or_default()
        );
    } else {
        println!("{secret}");
    }
    Ok(())
}

fn print_wallet_info(
    message: &str,
    address: &str,
    alias: Option<String>,
    source: &str,
    is_default: Option<bool>,
    format: OutputFormat,
) {
    print_wallet_receipt(
        message,
        address,
        alias,
        source,
        is_default,
        Vec::new(),
        format,
    );
}

fn print_wallet_receipt(
    message: &str,
    address: &str,
    alias: Option<String>,
    source: &str,
    is_default: Option<bool>,
    config_keys_written: Vec<String>,
    format: OutputFormat,
) {
    let vault_path = crate::ows::ows_vault_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".hyperliquid").display().to_string())
                .unwrap_or_else(|| "~/.hyperliquid".to_string())
        });
    let info = WalletInfo {
        message: message.to_string(),
        address: address.to_string(),
        alias,
        source: source.to_string(),
        is_default,
        config_path: crate::config::config_file_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unavailable".to_string()),
        vault_path,
        config_keys_written,
    };
    crate::output::print_data_no_timing(&info, format);
}

fn print_ows_wallet_change(
    message: &str,
    wallet: &ows_lib::types::WalletInfo,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let (hl_addr, _) = crate::ows::hyperliquid_address_from_wallet(wallet)
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    print_wallet_info(
        message,
        &hl_addr,
        Some(wallet.name.clone()),
        "OWS wallet",
        None,
        format,
    );
    Ok(())
}

fn account_type_display_label(account_type: &str) -> &str {
    match account_type {
        "api-wallet" => "local signing account",
        "agent-wallet" => "API/agent wallet",
        "ows-wallet" => "OWS wallet",
        other => other,
    }
}

fn print_status(status: &str, message: &str, format: OutputFormat) {
    if format == OutputFormat::Json {
        let output = StatusOutput {
            status: status.to_string(),
            message: message.to_string(),
        };
        crate::output::print_data_no_timing(&output, format);
    } else {
        println!("{message}");
    }
}

/// Report a declined confirmation. Human runs get a stderr note; JSON keeps its status document.
fn print_declined(message: &str, format: OutputFormat) {
    if format == OutputFormat::Json {
        print_status("cancelled", message, format);
    } else {
        eprintln!("{message}");
    }
}

fn print_ows_wallet_choices(wallets: &[ows_lib::types::WalletInfo]) -> Result<(), anyhow::Error> {
    // Filter to wallets with a Hyperliquid/EVM account, matching account_ls behavior.
    let hl_wallets: Vec<_> = wallets
        .iter()
        .filter(|w| crate::ows::hyperliquid_address_from_wallet(w).is_ok())
        .collect();
    if hl_wallets.is_empty() {
        return Err(no_stored_accounts_error().into());
    }
    eprintln!("Stored wallets:");
    for wallet in hl_wallets {
        let (hl_addr, _) = crate::ows::hyperliquid_address_from_wallet(wallet).unwrap();
        eprintln!("  {}: {} ({})", wallet.id, wallet.name, hl_addr);
    }
    Ok(())
}

fn no_stored_accounts_error() -> CliError {
    CliError::Unsupported(
        "no stored wallets found; run hyperliquid setup or hyperliquid wallet import".to_string(),
    )
}

fn wallet_not_found(selector: &str) -> CliError {
    CliError::Unsupported(format!("wallet '{selector}' not found"))
}

/// Fail with missing input naming `input` when this run can't ask a question.
fn require_promptable(input: &str, format: OutputFormat) -> Result<(), CliError> {
    if format == OutputFormat::Json {
        return Err(CliError::MissingInput(format!(
            "Can't prompt for {input} because of JSON output. Pass {input} instead."
        )));
    }
    prompt::ensure_can_prompt(input)
}

fn prompt_private_key() -> Result<String, CliError> {
    prompt::read_secret("Private key: ", "a private key on stdin")
}

/// Quote a value for display in a copy-pasteable shell command.
fn shell_arg(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':'));
    if plain {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_output_hides_encrypted_private_key() {
        let output = AccountsOutput::new(vec![AccountRow {
            id: 1,
            alias: "main".to_string(),
            address: "0xabc".to_string(),
            master_address: None,
            agent_name: None,
            expires_at: None,
            account_type: "api-wallet".to_string(),
            is_default: true,
            created_at: "now".to_string(),
        }]);
        let rendered = crate::output::render(&output, OutputFormat::Json);

        assert!(rendered.contains("main"));
        assert!(!rendered.contains("private_key"));
        assert!(!rendered.contains("encrypted"));
    }

    #[test]
    fn shell_arg_quotes_only_when_needed() {
        assert_eq!(shell_arg("main"), "main");
        assert_eq!(shell_arg("my wallet"), "'my wallet'");
        assert_eq!(shell_arg("it's"), "'it'\\''s'");
    }

    #[test]
    fn json_runs_name_the_missing_selector_without_prompting() {
        let err = require_promptable("<WALLET>", OutputFormat::Json).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("<WALLET>"));
    }
}
