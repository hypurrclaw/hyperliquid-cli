//! Wallet and account management commands.
//!
//! OWS (Open Wallet Standard) is the only wallet backend: creation, import,
//! listing and signing all go through the OWS vault (default `~/.hyperliquid`,
//! or `HYPERLIQUID_OWS_VAULT_PATH`). The `wallet` and `account` command families
//! are two surfaces over the same [`WalletStore`] workflow.

mod output;

pub use output::{AccountRow, AccountsOutput};

use clap::Args;
use ows_lib::types::WalletInfo;

use crate::auth::{ResolvedSigner, SignerSelection, SignerSource};
use crate::command_spec::{
    CliCommand, CommandSpec, DryRunPolicy, GroupSpec, InputKind, Lifecycle, NoArgs, OwsSupport,
    Risk, arg,
};
use crate::commands::support::print_dry_run;
use crate::commands::{builder, referral};
use crate::config::Config;
use crate::errors::CliError;
use crate::invocation::Invocation;
use crate::mode::Mode;
use crate::output::{self as out, OutputFormat};
use crate::ows::vault::{HyperliquidAccount, Vault};
use crate::prompt;
use crate::settings::Settings;
use crate::wallet_store::{ConfigEdit, NewWallet, StoredWallet, WalletName, WalletStore};
use output::{
    Receipt, print_declined, print_receipt, print_status, print_wallet_address, shell_arg,
};

const OWS_WALLET_SOURCE: &str = "OWS wallet";

pub(crate) fn prompt_private_key(mode: &Mode) -> Result<String, CliError> {
    prompt::read_secret(mode, "Private key: ", "a private key on stdin")
}

/// Fail with missing input naming `input` when this run can't ask a question.
fn require_promptable(input: &str, format: OutputFormat, mode: &Mode) -> Result<(), CliError> {
    if format == OutputFormat::Json {
        return Err(CliError::MissingInput(format!(
            "Can't prompt for {input} because of JSON output. Pass {input} instead."
        )));
    }
    prompt::ensure_can_prompt(mode, input)
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

fn print_new_wallet(inv: &Invocation, store: &WalletStore, message: &str, new: StoredWallet) {
    let mut receipt = Receipt::new(
        message,
        &new.account.address_text,
        Some(new.info.name),
        "stored OWS wallet",
    );
    receipt.is_default = Some(true);
    receipt.config_keys_written = new.config_keys_written;
    print_receipt(inv, store.settings(), store.vault(), receipt);
}

fn print_wallet_change(
    inv: &Invocation,
    store: &WalletStore,
    message: &str,
    wallet: &WalletInfo,
) -> Result<(), anyhow::Error> {
    let account = WalletStore::account_of(wallet)?;
    print_wallet_change_with(inv, store, message, wallet, &account);
    Ok(())
}

fn print_wallet_change_with(
    inv: &Invocation,
    store: &WalletStore,
    message: &str,
    wallet: &WalletInfo,
    account: &HyperliquidAccount,
) {
    let receipt = Receipt::new(
        message,
        &account.address_text,
        Some(wallet.name.clone()),
        OWS_WALLET_SOURCE,
    );
    print_receipt(inv, store.settings(), store.vault(), receipt);
}

pub fn create(inv: &Invocation) -> Result<(), anyhow::Error> {
    let store = WalletStore::open();
    let new = store.add(
        &NewWallet::Create,
        &WalletName::Next("wallet"),
        Some(adopt_as_default(store.settings())),
    )?;
    print_new_wallet(inv, &store, "Created wallet", new);
    Ok(())
}

pub fn import(inv: &Invocation, private_key: Option<&str>) -> Result<(), anyhow::Error> {
    let private_key = match private_key {
        Some(private_key) => {
            out::warn(
                "passing a private key as an argument can expose it in OS process listings and shell history. Prefer `hyperliquid wallet import` and paste at the hidden prompt.",
            );
            private_key.trim().to_string()
        }
        None => prompt_private_key(inv.mode())?.trim().to_string(),
    };

    let store = WalletStore::open();
    let new = store.add(
        &NewWallet::ImportPrivateKey(&private_key),
        &WalletName::Next("imported"),
        Some(adopt_as_default(store.settings())),
    )?;
    print_new_wallet(inv, &store, "Imported wallet", new);
    Ok(())
}

pub fn import_mnemonic(
    inv: &Invocation,
    mnemonic: Option<&str>,
    alias: Option<&str>,
) -> Result<(), anyhow::Error> {
    let mnemonic = match mnemonic {
        Some(mnemonic) => {
            out::warn(
                "passing a mnemonic as an argument can expose it in OS process listings and shell history. Prefer `hyperliquid wallet import-mnemonic` and paste at the hidden prompt.",
            );
            mnemonic.trim().to_string()
        }
        None => prompt::read_secret(
            inv.mode(),
            "Mnemonic phrase: ",
            "a mnemonic phrase on stdin",
        )?
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
    print_new_wallet(inv, &store, "Imported wallet", new);
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
    inv: &Invocation,
    settings: &Settings,
    selection: &SignerSelection,
    signer: &ResolvedSigner,
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
    print_receipt(inv, settings, &vault, receipt);
}

pub fn address(inv: &Invocation, signer: &ResolvedSigner) {
    print_wallet_address(inv, &signer.address().to_string());
}

pub fn reset(inv: &Invocation, yes: bool) -> Result<(), anyhow::Error> {
    let settings = Settings::from_process();
    let Some(file) = settings.config_file().filter(|file| file.exists()) else {
        print_status(inv, "noop", "Nothing to reset");
        return Ok(());
    };
    let path = file.path().display();

    if !yes {
        require_promptable("--yes", inv.format(), inv.mode())?;
        eprintln!("This deletes the config file at {path}.");
        eprintln!(
            "It clears the default wallet, network, builder, and referral defaults. Wallets stay in the vault."
        );
        if file.read().is_err() {
            eprintln!("The file isn't valid config, so its contents are discarded.");
        }
        let question = format!("Delete config file {path}?");
        if !prompt::confirm(inv.mode(), &question, "--yes")? {
            print_declined(inv, "Reset cancelled");
            return Ok(());
        }
    }

    let _lock = file.lock()?;
    file.remove()?;
    print_status(inv, "reset", &format!("Deleted config file {path}"));
    Ok(())
}

pub fn account_add(
    inv: &Invocation,
    private_key: Option<&str>,
    alias: Option<&str>,
    _account_type: Option<&str>,
    make_default: bool,
) -> Result<(), anyhow::Error> {
    let automation_mode = private_key.is_some() || alias.is_some() || make_default;
    let private_key = match private_key {
        Some(private_key) => {
            out::warn(
                "passing a private key as an argument can expose it in OS process listings and shell history.",
            );
            private_key.trim().to_string()
        }
        None => prompt_private_key(inv.mode())?.trim().to_string(),
    };

    let store = WalletStore::open();
    let name = match alias {
        Some(alias) => alias.trim().to_string(),
        None if automation_mode => String::new(),
        None => {
            require_promptable("--alias", inv.format(), inv.mode())?;
            prompt::read_line(inv.mode(), "Wallet name: ", "--alias")?
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
    print_wallet_change_with(inv, &store, "Account added", &stored.info, &stored.account);
    Ok(())
}

pub fn account_ls(inv: &Invocation) -> Result<(), anyhow::Error> {
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

    inv.print_untimed(&AccountsOutput::new(rows));
    Ok(())
}

/// Pick one stored wallet by selector, listing the choices when none was given.
fn choose_wallet(
    inv: &Invocation,
    store: &WalletStore,
    selector: Option<&str>,
) -> Result<WalletInfo, anyhow::Error> {
    if selector.is_none() {
        require_promptable("<WALLET>", inv.format(), inv.mode())?;
    }
    let wallets = store.list()?;
    if wallets.is_empty() {
        return Err(no_stored_accounts_error().into());
    }
    let selector = match selector {
        Some(selector) => selector.trim().to_string(),
        None => {
            print_wallet_choices(&wallets)?;
            prompt::read_line(inv.mode(), "Select wallet by name or id: ", "<WALLET>")?
                .trim()
                .to_string()
        }
    };
    Ok(WalletStore::find(wallets, &selector)?)
}

pub fn account_set_default(inv: &Invocation, selector: Option<&str>) -> Result<(), anyhow::Error> {
    let store = WalletStore::open();
    let wallet = choose_wallet(inv, &store, selector)?;
    store.set_default(&wallet)?;
    print_wallet_change(inv, &store, "Default account set", &wallet)
}

pub fn account_remove(
    inv: &Invocation,
    selector: Option<&str>,
    yes: bool,
) -> Result<(), anyhow::Error> {
    let store = WalletStore::open();
    let wallet = choose_wallet(inv, &store, selector)?;
    let address = WalletStore::account_of(&wallet)
        .map(|account| account.address_text)
        .unwrap_or_else(|_| "no Hyperliquid account".to_string());
    let is_default = store.is_configured_default(&wallet)?;

    if !yes {
        require_promptable("--yes", inv.format(), inv.mode())?;
        eprintln!("Wallet:  {}", wallet.name);
        eprintln!("Address: {address}");
        eprintln!("Default: {}", if is_default { "yes" } else { "no" });
        eprintln!();
        eprintln!("This deletes the encrypted key from the vault. This cannot be undone.");
        eprintln!("Back up first:");
        eprintln!("    hyperliquid wallet export {}", shell_arg(&wallet.name));
        eprintln!();
        let confirmed = if out::terminal::stdin_is_terminal() {
            prompt::confirm_typed(
                inv.mode(),
                &format!("To confirm, type the wallet name \"{}\": ", wallet.name),
                &wallet.name,
                "--yes",
            )?
        } else {
            prompt::confirm(
                inv.mode(),
                &format!("Delete wallet {}?", wallet.name),
                "--yes",
            )?
        };
        if !confirmed {
            print_declined(inv, "Delete cancelled");
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
    print_receipt(inv, store.settings(), store.vault(), receipt);
    Ok(())
}

/// List all wallets in the OWS vault.
pub fn list(inv: &Invocation) -> Result<(), anyhow::Error> {
    account_ls(inv)
}

/// Delete an OWS wallet by name or id.
pub fn delete(inv: &Invocation, selector: &str, yes: bool) -> Result<(), anyhow::Error> {
    account_remove(inv, Some(selector), yes)
}

pub fn rename(inv: &Invocation, selector: &str, new_name: &str) -> Result<(), anyhow::Error> {
    let store = WalletStore::open();
    let wallet = store.rename(selector, new_name)?;
    print_wallet_change(inv, &store, "Wallet renamed", &wallet)
}

/// Export an OWS wallet's secret.
///
/// NOTE: This command intentionally outputs plaintext private keys or mnemonics
/// to stdout, which is an exception to the AGENTS.md rule "Never log, print,
/// commit, or store plaintext private keys." The export is gated behind an
/// explicit interactive confirmation prompt (or --yes flag) and a warning
/// banner. The command's sole purpose is secret recovery/export.
pub fn export(inv: &Invocation, selector: &str, yes: bool) -> Result<(), anyhow::Error> {
    if !yes {
        require_promptable("--yes", inv.format(), inv.mode())?;
        let question = format!(
            "Export secret for wallet '{selector}'? This will display the private key or mnemonic."
        );
        if !prompt::confirm(inv.mode(), &question, "--yes")? {
            print_declined(inv, "Export cancelled");
            return Ok(());
        }
    }

    let store = WalletStore::open();
    let secret = store.export(selector)?;

    out::warn("this output contains a plaintext private key or mnemonic. Do not share or log it.");

    if inv.format() == OutputFormat::Json {
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

pub const GROUP: GroupSpec = GroupSpec::new(&["wallet"], "Manage wallets").aliases(&["wallets"]);

#[derive(Args, Debug, Clone)]
pub struct AccountAddArgs {
    /// Private key to store (0x-prefixed hex). Omit to use the hidden prompt.
    pub private_key: Option<String>,
    /// Alias for the wallet. Defaults to the derived address in non-interactive mode.
    #[arg(long)]
    pub alias: Option<String>,
    /// Wallet type.
    #[arg(long = "type")]
    pub account_type: Option<String>,
    /// Make this wallet the default signer.
    #[arg(long)]
    pub default: bool,
}

#[derive(Args, Debug, Clone)]
pub struct AccountSetDefaultArgs {
    /// Wallet name or id. Omit to choose interactively.
    pub selector: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct AccountRemoveArgs {
    /// Wallet name or id. Omit to choose interactively.
    pub selector: Option<String>,
    /// Confirm removal without prompting.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Args, Debug, Clone)]
pub struct WalletImportArgs {
    /// Private key to import (0x-prefixed hex). Omit to use the hidden prompt.
    pub private_key: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct WalletImportMnemonicArgs {
    /// BIP-39 mnemonic phrase. Omit to use the hidden prompt.
    pub mnemonic: Option<String>,
    /// Alias/name for the imported wallet.
    #[arg(long)]
    pub alias: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct WalletRenameArgs {
    /// Wallet name or id to rename.
    pub selector: String,
    /// New name for the wallet.
    pub new_name: String,
}

#[derive(Args, Debug, Clone)]
pub struct WalletDeleteArgs {
    /// Wallet name or id to delete.
    pub selector: String,
    /// Confirm deletion without prompting.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Args, Debug, Clone)]
pub struct WalletExportArgs {
    /// Wallet name or id to export.
    pub selector: String,
    /// Skip confirmation prompt.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Args, Debug, Clone)]
pub struct WalletResetArgs {
    /// Confirm reset without prompting.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

pub struct AccountAdd;

impl CliCommand for AccountAdd {
    type Args = AccountAddArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["account", "add"],
        "wallet",
        "Add an OWS wallet",
        "Add a signing wallet. Passing private_key exposes it to the child process argv.",
    )
    .long_about("Add an OWS wallet.\n\
\n\
Recommended for humans: run `hyperliquid account add` without PRIVATE_KEY and paste the key at the hidden prompt.\n\
\n\
For controlled automation, pass PRIVATE_KEY with --alias, --type, and optionally --default. Passing a private key as an argument can expose it in OS process listings and shell history.")
    .lifecycle(Lifecycle::InteractiveLocal)
    .risk(Risk::LocalSecret)
    .dry_run(DryRunPolicy::Optional)
    .ows(OwsSupport::NotApplicable)
    .dangerous(true)
    .prompt()
    .local()
    .args(&[
        arg("private_key").describe("0x-prefixed private key. Omit in human contexts to use the hidden prompt; argv secrets are rejected in machine contexts.").secret(),
        arg("alias").describe("Alias for the wallet.").resource_id("account alias"),
        arg("account_type"),
        arg("default").kind(InputKind::Boolean),
    ]);

    async fn run(inv: &Invocation, args: &AccountAddArgs) -> Result<(), anyhow::Error> {
        if inv.dry_run() {
            return print_dry_run(
                inv,
                "account add",
                serde_json::json!({
                    "would_execute": "store_encrypted_signing_account",
                    "args": {
                        "private_key_supplied": args.private_key.is_some(),
                        "alias": args.alias,
                        "type": args.account_type,
                        "default": args.default
                    }
                }),
            );
        }
        account_add(
            inv,
            args.private_key.as_deref(),
            args.alias.as_deref(),
            args.account_type.as_deref(),
            args.default,
        )
    }
}

pub struct AccountLs;

impl CliCommand for AccountLs {
    type Args = NoArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["account", "ls"],
        "wallet",
        "List wallets",
        "List wallets in the OWS vault.",
    )
    .risk(Risk::LocalState)
    .ows(OwsSupport::NotApplicable)
    .local()
    .cli_aliases(&["list"]);

    async fn run(inv: &Invocation, _args: &NoArgs) -> Result<(), anyhow::Error> {
        account_ls(inv)
    }
}

pub struct AccountSetDefault;

impl CliCommand for AccountSetDefault {
    type Args = AccountSetDefaultArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["account", "set-default"],
        "wallet",
        "Set the default wallet",
        "Set the default signing wallet.",
    )
    .lifecycle(Lifecycle::InteractiveLocal)
    .risk(Risk::LocalState)
    .dry_run(DryRunPolicy::Optional)
    .ows(OwsSupport::NotApplicable)
    .dangerous(true)
    .local()
    .args(&[arg("selector")
        .describe("Wallet name, wallet id, or address.")
        .resource_id("account selector")
        .schema_required()]);

    async fn run(inv: &Invocation, args: &AccountSetDefaultArgs) -> Result<(), anyhow::Error> {
        if inv.dry_run() {
            return print_dry_run(
                inv,
                "account set-default",
                serde_json::json!({
                    "would_execute": "set_default_account",
                    "args": {"selector": args.selector}
                }),
            );
        }
        account_set_default(inv, args.selector.as_deref())
    }
}

pub struct AccountRemove;

impl CliCommand for AccountRemove {
    type Args = AccountRemoveArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["account", "remove"],
        "wallet",
        "Delete a wallet and its stored key",
        "Delete a wallet's encrypted key from the vault. This cannot be undone.",
    )
    .lifecycle(Lifecycle::InteractiveLocal)
    .risk(Risk::LocalState)
    .dry_run(DryRunPolicy::Optional)
    .ows(OwsSupport::NotApplicable)
    .dangerous(true)
    .prompt()
    .confirms_with_arg("selector")
    .local()
    .args(&[
        arg("selector")
            .describe("Wallet name, wallet id, or address.")
            .resource_id("account selector")
            .schema_required(),
        arg("yes")
            .kind(InputKind::Boolean)
            .describe("Skip confirmation prompt for deliberate automation."),
    ]);

    async fn run(inv: &Invocation, args: &AccountRemoveArgs) -> Result<(), anyhow::Error> {
        if inv.dry_run() {
            return print_dry_run(
                inv,
                "account remove",
                serde_json::json!({
                    "would_execute": "remove_stored_account",
                    "args": {"selector": args.selector, "yes": args.yes}
                }),
            );
        }
        account_remove(inv, args.selector.as_deref(), args.yes)
    }
}

pub struct WalletCreate;

impl CliCommand for WalletCreate {
    type Args = NoArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["wallet", "create"],
        "wallet",
        "Create a wallet",
        "Create a new wallet in the OWS vault.",
    )
    .lifecycle(Lifecycle::InteractiveLocal)
    .risk(Risk::LocalSecret)
    .dry_run(DryRunPolicy::Optional)
    .ows(OwsSupport::NotApplicable)
    .dangerous(true)
    .local();

    async fn run(inv: &Invocation, _args: &NoArgs) -> Result<(), anyhow::Error> {
        if inv.dry_run() {
            return print_dry_run(
                inv,
                "wallet create",
                serde_json::json!({"would_execute": "create_and_store_encrypted_wallet"}),
            );
        }
        create(inv)
    }
}

pub struct WalletImport;

impl CliCommand for WalletImport {
    type Args = WalletImportArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["wallet", "import"],
        "wallet",
        "Import an existing private key",
        "Import a private key into the OWS vault. Passing private_key exposes it to the child process argv.",
    )
    .long_about("Import an existing private key.\n\
\n\
Recommended for humans: run `hyperliquid wallet import` without PRIVATE_KEY and paste the key at the hidden prompt.\n\
\n\
Warning: `hyperliquid wallet import <PRIVATE_KEY>` can expose the key in OS process listings and shell history. Use the argument form only for controlled automation.")
    .lifecycle(Lifecycle::InteractiveLocal)
    .risk(Risk::LocalSecret)
    .dry_run(DryRunPolicy::Optional)
    .ows(OwsSupport::NotApplicable)
    .dangerous(true)
    .prompt()
    .local()
    .args(&[
        arg("private_key").describe("0x-prefixed private key. Omit in human contexts to use the hidden prompt; argv secrets are rejected in machine contexts.").secret(),
    ]);

    async fn run(inv: &Invocation, args: &WalletImportArgs) -> Result<(), anyhow::Error> {
        if inv.dry_run() {
            return print_dry_run(
                inv,
                "wallet import",
                serde_json::json!({
                    "would_execute": "validate_and_store_encrypted_wallet",
                    "args": {"private_key_supplied": args.private_key.is_some()}
                }),
            );
        }
        import(inv, args.private_key.as_deref())
    }
}

pub struct WalletImportMnemonic;

impl CliCommand for WalletImportMnemonic {
    type Args = WalletImportMnemonicArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["wallet", "import-mnemonic"],
        "wallet",
        "Import an existing wallet from a BIP-39 mnemonic phrase",
        "Import an existing wallet from a BIP-39 mnemonic phrase.",
    )
    .long_about("Import an existing wallet from a BIP-39 mnemonic phrase.\n\
\n\
Recommended for humans: run `hyperliquid wallet import-mnemonic` without MNEMONIC and paste the phrase at the hidden prompt.\n\
\n\
Warning: passing a mnemonic as an argument can expose it in OS process listings and shell history. Use the argument form only for controlled automation.")
    .lifecycle(Lifecycle::InteractiveLocal)
    .risk(Risk::LocalSecret)
    .dry_run(DryRunPolicy::Optional)
    .ows(OwsSupport::NotApplicable)
    .dangerous(true)
    .prompt()
    .local()
    .args(&[
        arg("mnemonic").describe("BIP-39 mnemonic phrase. Omit in human contexts to use the hidden prompt; argv secrets are rejected in machine contexts.").secret(),
        arg("alias"),
    ]);

    async fn run(inv: &Invocation, args: &WalletImportMnemonicArgs) -> Result<(), anyhow::Error> {
        if inv.dry_run() {
            return print_dry_run(
                inv,
                "wallet import-mnemonic",
                serde_json::json!({
                    "would_execute": "import_mnemonic_to_ows_vault",
                    "args": {"mnemonic_supplied": args.mnemonic.is_some(), "alias": args.alias}
                }),
            );
        }
        import_mnemonic(inv, args.mnemonic.as_deref(), args.alias.as_deref())
    }
}

pub struct WalletShow;

impl CliCommand for WalletShow {
    type Args = NoArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["wallet", "show"],
        "wallet",
        "Show the current wallet",
        "Show current wallet metadata.",
    )
    .risk(Risk::LocalSecret)
    .ows(OwsSupport::AddressSelectorSupported)
    .auth(true);

    async fn run(inv: &Invocation, _args: &NoArgs) -> Result<(), anyhow::Error> {
        let signer = inv.resolve_signer()?;
        let remote = inv.remote()?;
        show(inv, remote.settings(), remote.selection(), &signer);
        Ok(())
    }
}

pub struct WalletAddress;

impl CliCommand for WalletAddress {
    type Args = NoArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["wallet", "address"],
        "wallet",
        "Print the wallet address",
        "Print the configured wallet address.",
    )
    .risk(Risk::LocalSecret)
    .ows(OwsSupport::AddressSelectorSupported)
    .auth(true);

    async fn run(inv: &Invocation, _args: &NoArgs) -> Result<(), anyhow::Error> {
        let signer = inv.resolve_signer()?;
        address(inv, &signer);
        Ok(())
    }
}

pub struct WalletList;

impl CliCommand for WalletList {
    type Args = NoArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["wallet", "list"],
        "wallet",
        "List all wallets",
        "List all wallets in the OWS vault.",
    )
    .risk(Risk::LocalState)
    .ows(OwsSupport::NotApplicable)
    .local()
    .cli_aliases(&["ls"]);

    async fn run(inv: &Invocation, _args: &NoArgs) -> Result<(), anyhow::Error> {
        list(inv)
    }
}

pub struct WalletRename;

impl CliCommand for WalletRename {
    type Args = WalletRenameArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["wallet", "rename"],
        "wallet",
        "Rename a wallet",
        "Rename a wallet by name or id.",
    )
    .lifecycle(Lifecycle::InteractiveLocal)
    .risk(Risk::LocalState)
    .ows(OwsSupport::NotApplicable)
    .dangerous(true)
    .local()
    .dry_run_refusal("--dry-run is not supported for wallet rename")
    .args(&[arg("selector"), arg("new_name")]);

    async fn run(inv: &Invocation, args: &WalletRenameArgs) -> Result<(), anyhow::Error> {
        rename(inv, &args.selector, &args.new_name)
    }
}

pub struct WalletDelete;

impl CliCommand for WalletDelete {
    type Args = WalletDeleteArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["wallet", "delete"],
        "wallet",
        "Delete a wallet and its stored key",
        "Delete a wallet's encrypted key from the vault. This cannot be undone.",
    )
    .lifecycle(Lifecycle::InteractiveLocal)
    .risk(Risk::LocalState)
    .dry_run(DryRunPolicy::Optional)
    .ows(OwsSupport::NotApplicable)
    .dangerous(true)
    .prompt()
    .local()
    .args(&[arg("selector"), arg("yes").kind(InputKind::Boolean)]);

    async fn run(inv: &Invocation, args: &WalletDeleteArgs) -> Result<(), anyhow::Error> {
        if inv.dry_run() {
            return print_dry_run(
                inv,
                "wallet delete",
                serde_json::json!({
                    "would_execute": "delete_ows_wallet",
                    "args": {"selector": args.selector, "yes": args.yes}
                }),
            );
        }
        delete(inv, &args.selector, args.yes)
    }
}

pub struct WalletExport;

impl CliCommand for WalletExport {
    type Args = WalletExportArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["wallet", "export"],
        "wallet",
        "Export a wallet's secret (mnemonic or private key)",
        "Export a wallet's secret (mnemonic or private key).",
    )
    .lifecycle(Lifecycle::InteractiveLocal)
    .risk(Risk::LocalSecret)
    .ows(OwsSupport::NotApplicable)
    .dangerous(true)
    .prompt()
    .local()
    .dry_run_refusal("--dry-run is not supported for wallet export")
    .args(&[arg("selector"), arg("yes").kind(InputKind::Boolean)]);

    async fn run(inv: &Invocation, args: &WalletExportArgs) -> Result<(), anyhow::Error> {
        export(inv, &args.selector, args.yes)
    }
}

pub struct WalletReset;

impl CliCommand for WalletReset {
    type Args = WalletResetArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["wallet", "reset"],
        "wallet",
        "Delete the wallet config file and default wallet reference",
        "Delete the config file, clearing default wallet, network, and builder and referral defaults. Wallets stay in the vault.",
    )
    .lifecycle(Lifecycle::InteractiveLocal)
    .risk(Risk::LocalState)
    .dry_run(DryRunPolicy::Optional)
    .ows(OwsSupport::NotApplicable)
    .dangerous(true)
    .prompt()
    .local()
    .args(&[
        arg("yes").kind(InputKind::Boolean).describe("Skip confirmation prompt for deliberate automation."),
    ]);

    async fn run(inv: &Invocation, args: &WalletResetArgs) -> Result<(), anyhow::Error> {
        if inv.dry_run() {
            return print_dry_run(
                inv,
                "wallet reset",
                serde_json::json!({
                    "would_execute": "remove_wallet_configuration",
                    "args": {"yes": args.yes}
                }),
            );
        }
        reset(inv, args.yes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_runs_name_the_missing_selector_without_prompting() {
        let err = require_promptable("<WALLET>", OutputFormat::Json, &Mode::default()).unwrap_err();
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
