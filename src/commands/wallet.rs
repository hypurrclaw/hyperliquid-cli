//! Wallet and local account management commands.
//!
//! OWS (Open Wallet Standard) is the primary and only wallet backend.
//! All wallet creation, import, listing, and signing flow through the OWS vault
//! at ~/.hyperliquid (or HYPERLIQUID_OWS_VAULT_PATH when set).

use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

use crate::auth::{self, SignerSource};
use crate::config::{self, Config};
use crate::errors::CliError;
use crate::output::{OutputFormat, TableData};

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
        if self.accounts.is_empty() {
            vec!["Message"]
        } else {
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
    }

    fn rows(&self) -> Vec<Vec<String>> {
        if self.accounts.is_empty() {
            return vec![vec!["no accounts found".to_string()]];
        }

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
}

impl TableData for WalletInfo {
    fn headers(&self) -> Vec<&str> {
        vec!["Field", "Value"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![
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
        ]
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

pub fn create(format: OutputFormat) -> Result<(), anyhow::Error> {
    let passphrase = crate::ows::ows_passphrase();
    let vault_path = crate::ows::ows_vault_path();
    let name = next_ows_name("wallet", vault_path.as_deref())?;

    // Load config before wallet creation so a corrupt config doesn't leave an orphan.
    let mut cfg = config::load_config()?.unwrap_or_default();
    apply_setup_defaults_to_config(&mut cfg);

    let wallet = crate::ows::create_ows_wallet(&name, passphrase.as_deref(), vault_path.as_deref())
        .map_err(|err| anyhow::anyhow!("{err}"))?;

    let (hyperliquid_account, _) = crate::ows::hyperliquid_address_from_wallet(&wallet)
        .map_err(|err| anyhow::anyhow!("{err}"))?;

    // Mark the new wallet as default in config.
    if let Some(ref prev) = cfg.default_wallet_id
        && prev != &wallet.id
    {
        eprintln!(
            "Note: default wallet changed from '{}' to '{}'",
            prev, wallet.id
        );
    }
    cfg.default_wallet_id = Some(wallet.id.clone());
    config::save_config(&cfg).map_err(|err| {
        let _ = crate::ows::delete_ows_wallet(&wallet.id, vault_path.as_deref());
        anyhow::anyhow!("failed to save config: {err}")
    })?;

    print_wallet_info(
        "Created wallet",
        &hyperliquid_account,
        Some(wallet.name.clone()),
        "stored OWS wallet",
        Some(true),
        format,
    );
    Ok(())
}

/// Apply packaged builder/referral defaults when the config does not already define them.
fn apply_setup_defaults_to_config(cfg: &mut Config) {
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
    }
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
            eprintln!(
                "Warning: passing a private key as an argument can expose it in OS process listings and shell history. Prefer `hyperliquid wallet import` and paste at the hidden prompt."
            );
            private_key.trim().to_string()
        }
        None => prompt_private_key(format)?.trim().to_string(),
    };

    let passphrase = crate::ows::ows_passphrase();
    let vault_path = crate::ows::ows_vault_path();
    let name = next_ows_name("imported", vault_path.as_deref())?;

    // Load config before wallet creation so a corrupt config doesn't leave an orphan.
    let mut cfg = config::load_config()?.unwrap_or_default();
    apply_setup_defaults_to_config(&mut cfg);

    let wallet = crate::ows::import_ows_wallet_private_key(
        &name,
        &private_key,
        passphrase.as_deref(),
        vault_path.as_deref(),
    )
    .map_err(|err| anyhow::anyhow!("{err}"))?;

    let (hyperliquid_account, _) = crate::ows::hyperliquid_address_from_wallet(&wallet)
        .map_err(|err| anyhow::anyhow!("{err}"))?;

    // Mark the new wallet as default in config.
    if let Some(ref prev) = cfg.default_wallet_id
        && prev != &wallet.id
    {
        eprintln!(
            "Note: default wallet changed from '{}' to '{}'",
            prev, wallet.id
        );
    }
    cfg.default_wallet_id = Some(wallet.id.clone());
    config::save_config(&cfg).map_err(|err| {
        let _ = crate::ows::delete_ows_wallet(&wallet.id, vault_path.as_deref());
        anyhow::anyhow!("failed to save config: {err}")
    })?;

    print_wallet_info(
        "Imported wallet",
        &hyperliquid_account,
        Some(wallet.name.clone()),
        "stored OWS wallet",
        Some(true),
        format,
    );
    Ok(())
}

pub fn import_mnemonic(
    mnemonic: Option<&str>,
    alias: Option<&str>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let mnemonic = match mnemonic {
        Some(mnemonic) => {
            eprintln!(
                "Warning: passing a mnemonic as an argument can expose it in OS process listings and shell history. Prefer `hyperliquid wallet import-mnemonic` and paste at the hidden prompt."
            );
            mnemonic.trim().to_string()
        }
        None => {
            use std::io::IsTerminal;
            if io::stdin().is_terminal() {
                rpassword::prompt_password("Mnemonic phrase: ")?
            } else {
                prompt("Mnemonic phrase: ", format)?
            }
        }
    };

    let passphrase = crate::ows::ows_passphrase();
    let vault_path = crate::ows::ows_vault_path();
    let name = match alias {
        Some(alias) => alias.trim().to_string(),
        None => next_ows_name("imported", vault_path.as_deref())?,
    };

    // Load config before wallet creation so a corrupt config doesn't leave an orphan.
    let mut cfg = config::load_config()?.unwrap_or_default();
    apply_setup_defaults_to_config(&mut cfg);

    let wallet = crate::ows::import_ows_wallet_mnemonic(
        &name,
        &mnemonic,
        passphrase.as_deref(),
        vault_path.as_deref(),
    )
    .map_err(|err| anyhow::anyhow!("{err}"))?;

    let (hyperliquid_account, _) = crate::ows::hyperliquid_address_from_wallet(&wallet)
        .map_err(|err| anyhow::anyhow!("{err}"))?;

    // Mark the new wallet as default in config.
    if let Some(ref prev) = cfg.default_wallet_id
        && prev != &wallet.id
    {
        eprintln!(
            "Note: default wallet changed from '{}' to '{}'",
            prev, wallet.id
        );
    }
    cfg.default_wallet_id = Some(wallet.id.clone());
    config::save_config(&cfg).map_err(|err| {
        let _ = crate::ows::delete_ows_wallet(&wallet.id, vault_path.as_deref());
        anyhow::anyhow!("failed to save config: {err}")
    })?;

    print_wallet_info(
        "Imported wallet",
        &hyperliquid_account,
        Some(wallet.name.clone()),
        "stored OWS wallet",
        Some(true),
        format,
    );
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
            let cfg = config::load_config().ok().flatten();
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
        let cfg = config::load_config().ok().flatten();
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
    if !wallet_configuration_exists() {
        print_status("noop", "Nothing to reset", format);
        return Ok(());
    }

    if !yes {
        write_prompt(
            "Reset wallet configuration and remove default wallet reference? [y/N] ",
            format,
        )?;
        if !read_confirmation()? {
            print_status("cancelled", "Reset cancelled", format);
            return Ok(());
        }
    }

    // Remove the config file (no need to save first since we're deleting it).
    if let Some(path) = crate::config::config_file_path()
        && path.exists()
    {
        std::fs::remove_file(&path).map_err(|err| {
            CliError::Configuration(format!("failed to remove config {}: {err}", path.display()))
        })?;
    }
    print_status("reset", "Wallet configuration reset", format);
    Ok(())
}

fn wallet_configuration_exists() -> bool {
    // Check for config file or default wallet reference.
    if let Ok(Some(cfg)) = config::load_config()
        && cfg.default_wallet_id.is_some()
    {
        return true;
    }
    crate::config::config_file_path().is_some_and(|path| path.exists())
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
            eprintln!(
                "Warning: passing a private key as an argument can expose it in OS process listings and shell history."
            );
            private_key.trim().to_string()
        }
        None => prompt_private_key(format)?.trim().to_string(),
    };
    let name = match alias {
        Some(alias) => alias.trim().to_string(),
        None if automation_mode => {
            let vp = crate::ows::ows_vault_path();
            next_ows_name("account", vp.as_deref())?
        }
        None => prompt("Wallet name: ", format)?,
    };
    let name = if name.trim().is_empty() {
        let vp = crate::ows::ows_vault_path();
        next_ows_name("account", vp.as_deref())?
    } else {
        name.trim().to_string()
    };

    let passphrase = crate::ows::ows_passphrase();
    let vault_path = crate::ows::ows_vault_path();
    let wallet = crate::ows::import_ows_wallet_private_key(
        &name,
        &private_key,
        passphrase.as_deref(),
        vault_path.as_deref(),
    )
    .map_err(|err| anyhow::anyhow!("{err}"))?;

    if make_default {
        let mut cfg = config::load_config()?.unwrap_or_default();
        cfg.default_wallet_id = Some(wallet.id.clone());
        config::save_config(&cfg).map_err(|err| {
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
    let config = config::load_config().ok().flatten();
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
        let is_default = default_id.map_or(display_index == 0, |d| d == w.id);
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

    crate::output::print_data_no_timing(&AccountsOutput::new(all_rows), format);
    Ok(())
}

pub fn account_set_default(
    selector: Option<&str>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let vault_path = crate::ows::ows_vault_path();
    let wallets = crate::ows::list_ows_wallets(vault_path.as_deref())
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    if wallets.is_empty() {
        return Err(no_stored_accounts_error().into());
    }
    let selector = match selector {
        Some(selector) => selector.trim().to_string(),
        None => {
            print_ows_wallet_choices(&wallets, format)?;
            prompt("Select wallet by name or id: ", format)?
                .trim()
                .to_string()
        }
    };
    let wallet = wallets
        .into_iter()
        .find(|w| w.name == selector || w.id == selector)
        .ok_or_else(|| wallet_not_found(&selector))?;

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
    let vault_path = crate::ows::ows_vault_path();
    let wallets = crate::ows::list_ows_wallets(vault_path.as_deref())
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    if wallets.is_empty() {
        return Err(no_stored_accounts_error().into());
    }
    let selector = match selector {
        Some(selector) => selector.trim().to_string(),
        None => {
            print_ows_wallet_choices(&wallets, format)?;
            prompt("Select wallet by name or id: ", format)?
                .trim()
                .to_string()
        }
    };
    let wallet = wallets
        .into_iter()
        .find(|w| w.name == selector || w.id == selector)
        .ok_or_else(|| wallet_not_found(&selector))?;
    if !yes {
        write_prompt(
            &format!("Remove wallet '{}' ({})? [y/N] ", wallet.name, wallet.id),
            format,
        )?;
        if !read_confirmation()? {
            print_status("cancelled", "Remove cancelled", format);
            return Ok(());
        }
    }
    crate::ows::delete_ows_wallet(&wallet.id, vault_path.as_deref())
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    // Clear default_wallet_id if we just deleted the default wallet.
    if let Ok(Some(mut config)) = config::load_config()
        && config.default_wallet_id.as_deref() == Some(&wallet.id)
    {
        config.default_wallet_id = None;
        config::save_config(&config)?;
    }
    print_ows_wallet_change("Account removed", &wallet, format)?;
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
        write_prompt(
            &format!(
                "Export secret for wallet '{}'? This will display the private key/mnemonic. [y/N] ",
                selector
            ),
            format,
        )?;
        if !read_confirmation()? {
            print_status("cancelled", "Export cancelled", format);
            return Ok(());
        }
    }

    let passphrase = crate::ows::ows_passphrase();
    let vault_path = crate::ows::ows_vault_path();
    let secret =
        crate::ows::export_ows_wallet(selector, passphrase.as_deref(), vault_path.as_deref())
            .map_err(|err| anyhow::anyhow!("{err}"))?;

    eprintln!(
        "Warning: this output contains a plaintext private key or mnemonic. Do not share or log it."
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

fn print_ows_wallet_choices(
    wallets: &[ows_lib::types::WalletInfo],
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    // Filter to wallets with a Hyperliquid/EVM account, matching account_ls behavior.
    let hl_wallets: Vec<_> = wallets
        .iter()
        .filter(|w| crate::ows::hyperliquid_address_from_wallet(w).is_ok())
        .collect();
    if hl_wallets.is_empty() {
        return Err(no_stored_accounts_error().into());
    }
    write_line("Stored wallets:", format)?;
    for wallet in hl_wallets {
        let (hl_addr, _) = crate::ows::hyperliquid_address_from_wallet(wallet).unwrap();
        write_line(
            &format!("  {}: {} ({})", wallet.id, wallet.name, hl_addr),
            format,
        )?;
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

fn prompt(label: &str, format: OutputFormat) -> Result<String, anyhow::Error> {
    write_prompt(label, format)?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim_end_matches(['\r', '\n']).to_string())
}

fn prompt_private_key(format: OutputFormat) -> Result<String, anyhow::Error> {
    use std::io::IsTerminal;

    if io::stdin().is_terminal() {
        return Ok(rpassword::prompt_password("Private key: ")?);
    }

    prompt("Private key: ", format)
}

fn write_prompt(label: &str, format: OutputFormat) -> Result<(), anyhow::Error> {
    if format == OutputFormat::Json {
        eprint!("{label}");
        io::stderr().flush()?;
    } else {
        print!("{label}");
        io::stdout().flush()?;
    }
    Ok(())
}

fn write_line(line: &str, format: OutputFormat) -> Result<(), anyhow::Error> {
    if format == OutputFormat::Json {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
    Ok(())
}

fn read_confirmation() -> Result<bool, anyhow::Error> {
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(is_yes(input.trim()))
}

fn is_yes(input: &str) -> bool {
    matches!(input.to_ascii_lowercase().as_str(), "y" | "yes")
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
    fn yes_parser_accepts_only_affirmative_answers() {
        assert!(is_yes("y"));
        assert!(is_yes("yes"));
        assert!(is_yes("YES"));
        assert!(!is_yes(""));
        assert!(!is_yes("n"));
    }
}
