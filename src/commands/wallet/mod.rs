//! Wallet and account management commands.
//!
//! OWS (Open Wallet Standard) is the only wallet backend: creation, import,
//! listing and signing all go through the OWS vault (default `~/.hyperliquid`,
//! or `HYPERLIQUID_OWS_VAULT_PATH`). The `wallet` and `account` command families
//! are two surfaces over the same [`WalletStore`] workflow.

mod output;

pub use output::{AccountRow, AccountsOutput};

use ows_lib::types::WalletInfo;

use crate::auth::{ResolvedSigner, SignerSelection, SignerSource};
use crate::commands::{builder, referral};
use crate::config::Config;
use crate::errors::CliError;
use crate::output::OutputFormat;
use crate::ows::vault::{HyperliquidAccount, Vault};
use crate::prompt;
use crate::settings::Settings;
use crate::wallet_store::{ConfigEdit, NewWallet, StoredWallet, WalletName, WalletStore};
use output::{
    Receipt, print_declined, print_receipt, print_status, print_wallet_address, shell_arg,
};

const OWS_WALLET_SOURCE: &str = "OWS wallet";

pub(crate) fn prompt_private_key() -> Result<String, CliError> {
    prompt::read_secret("Private key: ", "a private key on stdin")
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

fn no_stored_accounts_error() -> CliError {
    CliError::Unsupported(
        "no stored wallets found; run hyperliquid setup or hyperliquid wallet import".to_string(),
    )
}

/// Apply packaged builder/referral defaults when the config does not already define them.
///
/// Resolution failures are ignored: a bad default must not block wallet
/// creation. Returns the config keys it wrote.
fn apply_setup_defaults(config: &mut Config, settings: &Settings) -> Vec<String> {
    let mut written = Vec::new();
    if !config.has_builder_default()
        && let Ok(Some((address, fee))) = builder::suggested_default_builder(settings)
    {
        config.default_builder_address = Some(address);
        config.default_builder_fee_rate = Some(fee);
        written.push("default_builder_address".to_string());
        written.push("default_builder_fee_rate".to_string());
    }
    if !config.has_referral_default()
        && let Ok(Some(code)) =
            referral::suggested_default_referral_code(settings, config.network.is_testnet())
    {
        config.default_referral_code = Some(code);
        written.push("default_referral_code".to_string());
    }
    written
}

/// `wallet create|import|import-mnemonic`: the new wallet becomes the default and
/// the config gains packaged builder/referral defaults it lacks.
fn adopt_as_default(settings: &Settings) -> ConfigEdit<'_> {
    Box::new(move |config, wallet, _| {
        let mut written = apply_setup_defaults(config, settings);
        if let Some(previous) = config.default_wallet_id.as_deref()
            && previous != wallet.id
        {
            eprintln!(
                "! Default wallet changed from '{previous}' to '{}'.",
                wallet.id
            );
        }
        config.default_wallet_id = Some(wallet.id.clone());
        written.insert(0, "default_wallet_id".to_string());
        written
    })
}

fn print_new_wallet(store: &WalletStore, message: &str, new: StoredWallet, format: OutputFormat) {
    let mut receipt = Receipt::new(
        message,
        &new.account.address_text,
        Some(new.info.name),
        "stored OWS wallet",
    );
    receipt.is_default = Some(true);
    receipt.config_keys_written = new.config_keys_written;
    print_receipt(store.settings(), store.vault(), receipt, format);
}

fn print_wallet_change(
    store: &WalletStore,
    message: &str,
    wallet: &WalletInfo,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let account = WalletStore::account_of(wallet)?;
    print_wallet_change_with(store, message, wallet, &account, format);
    Ok(())
}

fn print_wallet_change_with(
    store: &WalletStore,
    message: &str,
    wallet: &WalletInfo,
    account: &HyperliquidAccount,
    format: OutputFormat,
) {
    let receipt = Receipt::new(
        message,
        &account.address_text,
        Some(wallet.name.clone()),
        OWS_WALLET_SOURCE,
    );
    print_receipt(store.settings(), store.vault(), receipt, format);
}

pub fn create(format: OutputFormat) -> Result<(), anyhow::Error> {
    let store = WalletStore::open();
    let new = store.add(
        &NewWallet::Create,
        &WalletName::Next("wallet"),
        Some(adopt_as_default(store.settings())),
    )?;
    print_new_wallet(&store, "Created wallet", new, format);
    Ok(())
}

pub fn import(private_key: Option<&str>, format: OutputFormat) -> Result<(), anyhow::Error> {
    let private_key = match private_key {
        Some(private_key) => {
            eprintln!(
                "Warning: passing a private key as an argument can expose it in OS process listings and shell history. Prefer `hyperliquid wallet import` and paste at the hidden prompt."
            );
            private_key.trim().to_string()
        }
        None => prompt_private_key()?.trim().to_string(),
    };

    let store = WalletStore::open();
    let new = store.add(
        &NewWallet::ImportPrivateKey(&private_key),
        &WalletName::Next("imported"),
        Some(adopt_as_default(store.settings())),
    )?;
    print_new_wallet(&store, "Imported wallet", new, format);
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
        None => prompt::read_secret("Mnemonic phrase: ", "a mnemonic phrase on stdin")?
            .trim()
            .to_string(),
    };

    let store = WalletStore::open();
    let alias = alias.map(str::trim);
    let name = match alias {
        Some(alias) => WalletName::Exact(alias),
        None => WalletName::Next("imported"),
    };
    let new = store.add(
        &NewWallet::ImportMnemonic(&mnemonic),
        &name,
        Some(adopt_as_default(store.settings())),
    )?;
    print_new_wallet(&store, "Imported wallet", new, format);
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

/// `wallet show`: describe the resolved signer. A vault wallet picked by
/// `--ows-signer` or by default is shown by name; any other signer by its source.
pub fn show(
    settings: &Settings,
    selection: &SignerSelection,
    signer: &ResolvedSigner,
    format: OutputFormat,
) {
    let address = signer.address().to_string();
    let vault = Vault::from_settings(settings);
    let receipt = match signer.ows_wallet_name() {
        Some(name) if selection.names_wallet_directly() => Receipt::new(
            "Current wallet",
            &address,
            Some(name.to_string()),
            &format!("OWS wallet ({name})"),
        ),
        _ => {
            let (alias, source) = signer_display(signer.source());
            Receipt::new("Current wallet", &address, alias, &source)
        }
    };
    print_receipt(settings, &vault, receipt, format);
}

pub fn address(signer: &ResolvedSigner, format: OutputFormat) {
    print_wallet_address(&signer.address().to_string(), format);
}

pub fn reset(yes: bool, format: OutputFormat) -> Result<(), anyhow::Error> {
    let settings = Settings::from_process();
    let Some(file) = settings.config_file().filter(|file| file.exists()) else {
        print_status("noop", "Nothing to reset", format);
        return Ok(());
    };
    let path = file.path().display();

    if !yes {
        require_promptable("--yes", format)?;
        eprintln!("This deletes the config file at {path}.");
        eprintln!(
            "It clears the default wallet, network, builder, and referral defaults. Wallets stay in the vault."
        );
        if file.read().is_err() {
            eprintln!("The file isn't valid config, so its contents are discarded.");
        }
        let question = format!("Delete config file {path}?");
        if !prompt::confirm(&question, "--yes")? {
            print_declined("Reset cancelled", format);
            return Ok(());
        }
    }

    let _lock = file.lock()?;
    file.remove()?;
    print_status("reset", &format!("Deleted config file {path}"), format);
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
            eprintln!(
                "Warning: passing a private key as an argument can expose it in OS process listings and shell history."
            );
            private_key.trim().to_string()
        }
        None => prompt_private_key()?.trim().to_string(),
    };

    let store = WalletStore::open();
    let name = match alias {
        Some(alias) => alias.trim().to_string(),
        None if automation_mode => String::new(),
        None => {
            require_promptable("--alias", format)?;
            prompt::read_line("Wallet name: ", "--alias")?
                .trim()
                .to_string()
        }
    };
    let name = if name.is_empty() {
        WalletName::Next("account")
    } else {
        WalletName::Exact(&name)
    };

    let make_default_edit: Option<ConfigEdit<'_>> = make_default.then(|| {
        Box::new(
            |config: &mut Config, wallet: &WalletInfo, _: &HyperliquidAccount| {
                config.default_wallet_id = Some(wallet.id.clone());
                Vec::new()
            },
        ) as ConfigEdit<'_>
    });
    let stored = store.add(
        &NewWallet::ImportPrivateKey(&private_key),
        &name,
        make_default_edit,
    )?;
    print_wallet_change_with(
        &store,
        "Account added",
        &stored.info,
        &stored.account,
        format,
    );
    Ok(())
}

pub fn account_ls(format: OutputFormat) -> Result<(), anyhow::Error> {
    let store = WalletStore::open();
    let mut listed = Vec::new();
    for wallet in store.list()? {
        match Vault::hyperliquid_account(&wallet) {
            Ok(account) => listed.push((wallet, account)),
            // Only wallets that genuinely lack a Hyperliquid/EVM account are skipped;
            // malformed addresses and vault corruption propagate.
            Err(CliError::OwsNoChainAccount { .. }) => {}
            Err(err) => return Err(anyhow::anyhow!("{err}")),
        }
    }
    let wallets: Vec<WalletInfo> = listed.iter().map(|(wallet, _)| wallet.clone()).collect();
    let markers = store.default_markers(&wallets)?;
    let rows = listed
        .into_iter()
        .zip(markers)
        .enumerate()
        .map(|(index, ((wallet, account), is_default))| {
            AccountRow::for_wallet(index + 1, wallet, &account, is_default)
        })
        .collect();

    crate::output::print_data_no_timing(&AccountsOutput::new(rows), format);
    Ok(())
}

/// Pick one stored wallet by selector, listing the choices when none was given.
fn choose_wallet(
    store: &WalletStore,
    selector: Option<&str>,
    format: OutputFormat,
) -> Result<WalletInfo, anyhow::Error> {
    if selector.is_none() {
        require_promptable("<WALLET>", format)?;
    }
    let wallets = store.list()?;
    if wallets.is_empty() {
        return Err(no_stored_accounts_error().into());
    }
    let selector = match selector {
        Some(selector) => selector.trim().to_string(),
        None => {
            print_wallet_choices(&wallets)?;
            prompt::read_line("Select wallet by name or id: ", "<WALLET>")?
                .trim()
                .to_string()
        }
    };
    Ok(WalletStore::find(wallets, &selector)?)
}

pub fn account_set_default(
    selector: Option<&str>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let store = WalletStore::open();
    let wallet = choose_wallet(&store, selector, format)?;
    store.set_default(&wallet)?;
    print_wallet_change(&store, "Default account set", &wallet, format)
}

pub fn account_remove(
    selector: Option<&str>,
    yes: bool,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let store = WalletStore::open();
    let wallet = choose_wallet(&store, selector, format)?;
    let address = WalletStore::account_of(&wallet)
        .map(|account| account.address_text)
        .unwrap_or_else(|_| "no Hyperliquid account".to_string());
    let is_default = store.is_configured_default(&wallet)?;

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
        let confirmed = if prompt::stdin_is_terminal() {
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
    store.remove(&wallet, is_default)?;
    if is_default {
        eprintln!("! The deleted wallet was the default. Pick another:");
        eprintln!("    hyperliquid account set-default <WALLET>");
    }
    let receipt = Receipt::new(
        "Deleted wallet",
        &address,
        Some(wallet.name.clone()),
        "OWS wallet",
    );
    print_receipt(store.settings(), store.vault(), receipt, format);
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

pub fn rename(selector: &str, new_name: &str, format: OutputFormat) -> Result<(), anyhow::Error> {
    let store = WalletStore::open();
    let wallet = store.rename(selector, new_name)?;
    print_wallet_change(&store, "Wallet renamed", &wallet, format)
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

    let store = WalletStore::open();
    let secret = store.export(selector)?;

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

/// Print the Hyperliquid-capable wallets to choose from.
fn print_wallet_choices(wallets: &[WalletInfo]) -> Result<(), anyhow::Error> {
    let choices: Vec<_> = wallets
        .iter()
        .filter_map(|wallet| {
            Vault::hyperliquid_account(wallet)
                .ok()
                .map(|account| (wallet, account))
        })
        .collect();
    if choices.is_empty() {
        return Err(no_stored_accounts_error().into());
    }
    eprintln!("Stored wallets:");
    for (wallet, account) in choices {
        eprintln!(
            "  {}: {} ({})",
            wallet.id, wallet.name, account.address_text
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_runs_name_the_missing_selector_without_prompting() {
        let err = require_promptable("<WALLET>", OutputFormat::Json).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("<WALLET>"));
    }

    #[test]
    fn setup_defaults_fill_only_what_the_config_lacks() {
        let settings = Settings::detached(
            crate::settings::EnvSource::fixed([
                (
                    crate::settings::env::DEFAULT_BUILDER_ADDRESS,
                    "0x0000000000000000000000000000000000000abc",
                ),
                (crate::settings::env::DEFAULT_BUILDER_FEE_RATE, "0.001%"),
                (crate::settings::env::DEFAULT_REFERRAL_CODE, "FRIEND"),
            ]),
            None,
        );

        let mut empty = Config::default();
        let written = apply_setup_defaults(&mut empty, &settings);
        assert_eq!(
            written,
            [
                "default_builder_address",
                "default_builder_fee_rate",
                "default_referral_code"
            ]
        );
        assert_eq!(empty.default_builder_fee_rate.as_deref(), Some("0.001%"));
        assert_eq!(empty.default_referral_code.as_deref(), Some("FRIEND"));

        let mut configured = Config {
            default_builder_address: Some("0xexisting".to_string()),
            default_referral_code: Some("MINE".to_string()),
            ..Config::default()
        };
        assert!(apply_setup_defaults(&mut configured, &settings).is_empty());
        assert_eq!(configured.default_referral_code.as_deref(), Some("MINE"));
    }
}
