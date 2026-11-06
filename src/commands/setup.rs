//! Guided first-time setup wizard.
//!
//! Questions and narration go to stderr; only the final result goes to stdout.
//! Every answer is collected, and a summary printed, before the first change to
//! the vault or config.

use clap::Args;
use hypersdk::Address;
use hypersdk::hypercore::{Chain, HttpClient};
use serde::Serialize;

use crate::commands::map_api_error;
use crate::config::{self, Config, Network};
use crate::errors::CliError;
use crate::output::{OutputFormat, TableData};
use crate::prompt;

#[derive(Args, Debug, Clone)]
pub struct SetupArgs {
    /// Create a new wallet and accept default setup choices without prompting.
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Submit the configured default builder fee approval during setup without an extra prompt.
    #[arg(long, conflicts_with = "no_approve_builder")]
    pub approve_builder: bool,

    /// Skip default builder fee approval during setup, even with --yes.
    #[arg(long, conflicts_with = "approve_builder")]
    pub no_approve_builder: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct SetupSummary {
    message: String,
    address: String,
    wallet_name: String,
    network: String,
    config_path: String,
    vault_path: String,
    connection: String,
    default_builder: String,
    builder_approval: String,
    default_referral_code: String,
    config_keys_written: Vec<String>,
}

impl TableData for SetupSummary {
    fn headers(&self) -> Vec<&str> {
        vec!["Field", "Value"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![
            vec!["Message".to_string(), self.message.clone()],
            vec!["Address".to_string(), self.address.clone()],
            vec!["Wallet".to_string(), self.wallet_name.clone()],
            vec!["Network".to_string(), self.network.clone()],
            vec!["Config".to_string(), self.config_path.clone()],
            vec!["Vault".to_string(), self.vault_path.clone()],
            vec!["Connection".to_string(), self.connection.clone()],
            vec!["Default builder".to_string(), self.default_builder.clone()],
            vec![
                "Builder approval".to_string(),
                self.builder_approval.clone(),
            ],
            vec![
                "Default referral".to_string(),
                self.default_referral_code.clone(),
            ],
            vec![
                "Config keys written".to_string(),
                self.config_keys_written.join(", "),
            ],
        ]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// How the wallet is obtained.
enum WalletSource {
    Create,
    /// A private key read at the masked prompt or from stdin. Never printed.
    Import(String),
}

/// Fail before asking anything when this run can't answer prompts and `--yes` wasn't given.
fn ensure_setup_can_run(args: &SetupArgs, format: OutputFormat) -> Result<(), CliError> {
    if args.yes {
        return Ok(());
    }
    let blocker = if format == OutputFormat::Json {
        Some("JSON output")
    } else {
        crate::mode::prompt_blocker()
    };
    match blocker {
        None => Ok(()),
        Some(reason) => Err(CliError::MissingInput(format!(
            "Can't run setup with prompts because of {reason}. Accept the defaults instead:\n    hyperliquid setup --yes"
        ))),
    }
}

/// Run the interactive first-time setup wizard.
pub async fn run(
    default_network: Network,
    args: &SetupArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    ensure_setup_can_run(args, format)?;
    // A broken config is an error before any question is asked.
    config::load_config()?;

    eprintln!("Welcome to Hyperliquid CLI setup");
    eprintln!(
        "This wizard will create or import a wallet, save your local config, and test the API connection."
    );
    eprintln!();
    eprintln!("Wallet setup:");
    eprintln!("  1) Create new wallet");
    eprintln!("  2) Import existing wallet (private key)");

    let wallet_choice = if args.yes {
        "1".to_string()
    } else {
        prompt::read_line("Choose an option [1/2]: ", "--yes")?
    };
    let import_wallet = match wallet_choice.trim().to_ascii_lowercase().as_str() {
        "" | "1" | "create" | "c" => false,
        "2" | "import" | "i" => true,
        other => {
            return Err(CliError::Unsupported(format!(
                "setup option '{other}' is not supported; choose 1 to create or 2 to import"
            ))
            .into());
        }
    };
    let network = if args.yes {
        default_network
    } else {
        prompt_network(default_network)?
    };

    // Verify connection and collect fallible defaults before creating any local
    // wallet state so failures do not leave orphaned wallets behind.
    let connection = verify_connection(network).await?;
    eprintln!("{connection}");
    let builder_suggestion = setup_builder_default_suggestion()?;
    let builder_defaults = if args.yes {
        builder_suggestion.clone()
    } else {
        prompt_builder_defaults()?
    };
    let referral_suggestion = setup_referral_default_suggestion(network)?;
    let referral_code = if args.yes {
        referral_suggestion.clone()
    } else {
        prompt_referral_default(network)?
    };
    let wallet_source = if import_wallet {
        WalletSource::Import(prompt_private_key()?.trim().to_string())
    } else {
        WalletSource::Create
    };
    let approve_builder = resolve_builder_approval(args, builder_defaults.as_ref())?;

    let vault_path = crate::ows::ows_vault_path();
    let wallet_name = match wallet_source {
        WalletSource::Create => next_setup_name("setup", vault_path.as_deref())?,
        WalletSource::Import(_) => next_setup_name("setup-imported", vault_path.as_deref())?,
    };
    let config_path = config::config_file_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unavailable".to_string());
    let vault_path_str = vault_path
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".hyperliquid").display().to_string())
                .unwrap_or_else(|| "~/.hyperliquid".to_string())
        });

    let default_builder = builder_defaults
        .as_ref()
        .map(|(address, fee)| format!("{address} at {fee}"))
        .unwrap_or_else(|| "not configured".to_string());
    print_answers_summary(&AnswersSummary {
        wallet: match wallet_source {
            WalletSource::Create => format!("create a new wallet named {wallet_name}"),
            WalletSource::Import(_) => format!("import a private key as wallet {wallet_name}"),
        },
        network,
        config_path: &config_path,
        vault_path: &vault_path_str,
        default_builder: &default_builder,
        builder_approval: if builder_defaults.is_none() {
            "not configured"
        } else if approve_builder {
            "submit a signed approval to Hyperliquid"
        } else {
            "skip"
        },
        referral: referral_code.as_deref().unwrap_or("not configured"),
    });

    crate::ows::warn_if_vault_passphrase_unset();
    let passphrase = crate::ows::ows_passphrase();

    let lock = config::lock_config()?;
    let mut config = config::load_config()?.unwrap_or_default();
    let (wallet_name, wallet_id, address, signer_address) = match &wallet_source {
        WalletSource::Create => {
            let wallet = crate::ows::create_ows_wallet(
                &wallet_name,
                passphrase.as_deref(),
                vault_path.as_deref(),
            )
            .map_err(|err| anyhow::anyhow!("{err}"))?;
            let (hl, hl_address) = crate::ows::hyperliquid_address_from_wallet(&wallet)
                .map_err(|err| anyhow::anyhow!("{err}"))?;
            (wallet.name, wallet.id, hl, hl_address)
        }
        WalletSource::Import(private_key) => {
            let wallet = crate::ows::import_ows_wallet_private_key(
                &wallet_name,
                private_key,
                passphrase.as_deref(),
                vault_path.as_deref(),
            )
            .map_err(|err| anyhow::anyhow!("{err}"))?;
            let (hl, hl_address) = crate::ows::hyperliquid_address_from_wallet(&wallet)
                .map_err(|err| anyhow::anyhow!("{err}"))?;
            (wallet.name, wallet.id, hl, hl_address)
        }
    };

    let mut config_keys_written = vec!["network".to_string(), "default_wallet_id".to_string()];
    config.network = network;
    config.default_wallet_id = Some(wallet_id.clone());
    config.default_builder_address = builder_defaults
        .as_ref()
        .map(|(address, _)| address.clone());
    config.default_builder_fee_rate = builder_defaults.as_ref().map(|(_, fee)| fee.clone());
    config.default_referral_code = referral_code.clone();
    if builder_defaults.is_some() {
        config_keys_written.push("default_builder_address".to_string());
        config_keys_written.push("default_builder_fee_rate".to_string());
    }
    if referral_code.is_some() {
        config_keys_written.push("default_referral_code".to_string());
    }
    save_setup_config(&config, &wallet_id, vault_path.as_deref())?;
    drop(lock);
    eprintln!("Saved config to {config_path}");

    let builder_approval = maybe_approve_default_builder(
        network,
        approve_builder,
        builder_defaults.as_ref(),
        &wallet_id,
        signer_address,
        vault_path.clone(),
    )
    .await?;

    let default_referral_code = referral_code
        .clone()
        .unwrap_or_else(|| "not configured".to_string());

    let output = SetupSummary {
        message: "Setup complete".to_string(),
        address,
        wallet_name,
        network: network.to_string(),
        config_path,
        vault_path: vault_path_str,
        connection,
        default_builder,
        builder_approval,
        default_referral_code,
        config_keys_written,
    };
    crate::output::print_data_no_timing(&output, format);

    let replayable = matches!(wallet_source, WalletSource::Create)
        && builder_defaults == builder_suggestion
        && referral_code == referral_suggestion;
    if !args.yes {
        print_replay_command(
            replayable,
            default_network,
            network,
            builder_defaults.is_some() && !approve_builder,
        );
    }
    Ok(())
}

/// Save the setup config, removing the wallet again when the save fails so the
/// vault isn't left with a wallet that no config references.
fn save_setup_config(
    config: &Config,
    wallet_id: &str,
    vault_path: Option<&std::path::Path>,
) -> Result<(), anyhow::Error> {
    config::save_config(config).map_err(|err| {
        let _ = crate::ows::delete_ows_wallet(wallet_id, vault_path);
        anyhow::anyhow!("failed to save config: {err}")
    })
}

struct AnswersSummary<'a> {
    wallet: String,
    network: Network,
    config_path: &'a str,
    vault_path: &'a str,
    default_builder: &'a str,
    builder_approval: &'a str,
    referral: &'a str,
}

/// Print what setup is about to do, before anything changes.
fn print_answers_summary(summary: &AnswersSummary<'_>) {
    eprintln!();
    eprintln!("Setup will:");
    eprintln!("  Wallet:           {}", summary.wallet);
    eprintln!("  Network:          {}", summary.network);
    eprintln!("  Config file:      {}", summary.config_path);
    eprintln!("  Vault:            {}", summary.vault_path);
    eprintln!("  Default builder:  {}", summary.default_builder);
    eprintln!("  Builder approval: {}", summary.builder_approval);
    eprintln!("  Default referral: {}", summary.referral);
    eprintln!();
}

/// Print the non-interactive command that repeats this run, when the flags can express it.
fn print_replay_command(
    replayable: bool,
    default_network: Network,
    network: Network,
    skip_builder_approval: bool,
) {
    eprintln!();
    if !replayable {
        eprintln!(
            "Importing a wallet or choosing custom builder or referral values can't be repeated without prompts."
        );
        return;
    }
    let mut command = String::new();
    if network == Network::Mainnet && default_network == Network::Testnet {
        command.push_str("HYPERLIQUID_NETWORK=mainnet ");
    }
    command.push_str("hyperliquid ");
    if network == Network::Testnet {
        command.push_str("--testnet ");
    }
    command.push_str("setup --yes");
    if skip_builder_approval {
        command.push_str(" --no-approve-builder");
    }
    eprintln!("Run this again without prompts:");
    eprintln!("    {command}");
}

async fn verify_connection(network: Network) -> Result<String, anyhow::Error> {
    let client = http_client_for_network(network)?;
    let mids = client.all_mids(None).await.map_err(map_api_error)?;
    Ok(format!(
        "Test query succeeded ({} mid prices returned)",
        mids.len()
    ))
}

fn http_client_for_network(network: Network) -> Result<HttpClient, anyhow::Error> {
    let chain = match network {
        Network::Mainnet => Chain::Mainnet,
        Network::Testnet => Chain::Testnet,
    };
    let client = HttpClient::new(chain);
    if let Some(api_base_url) = config::resolve_api_base_url_override_for_network(network)? {
        Ok(client.with_url(api_base_url))
    } else {
        Ok(client)
    }
}

fn prompt_network(default_network: Network) -> Result<Network, anyhow::Error> {
    let default_is_testnet = default_network == Network::Testnet;
    let default_hint = if default_is_testnet { "Y/n" } else { "y/N" };
    let answer = prompt::read_line(&format!("Use testnet? [{default_hint}]: "), "--yes")?;
    match answer.trim().to_ascii_lowercase().as_str() {
        "" => Ok(default_network),
        "y" | "yes" => Ok(Network::Testnet),
        "n" | "no" => Ok(Network::Mainnet),
        other => Err(CliError::Unsupported(format!(
            "network answer '{other}' is not supported; answer yes or no"
        ))
        .into()),
    }
}

fn prompt_builder_defaults() -> Result<Option<(String, String)>, anyhow::Error> {
    let suggestion = setup_builder_default_suggestion()?;
    if let Some((address, fee)) = suggestion.as_ref() {
        eprintln!(
            "Default builder fee setup (optional; press Enter to use the suggested default, or type 'none' to skip):"
        );
        eprintln!("Suggested builder: {address} at {fee}");
    } else {
        eprintln!("Default builder fee setup (optional; press Enter to skip):");
    }

    let address_prompt = suggestion
        .as_ref()
        .map(|(address, _)| format!("Default builder address [{address}]: "))
        .unwrap_or_else(|| "Default builder address: ".to_string());
    let address = prompt::read_line(&address_prompt, "--yes")?;
    let address = address.trim();
    if address.eq_ignore_ascii_case("none") || address.eq_ignore_ascii_case("skip") {
        return Ok(None);
    }
    let parsed = if address.is_empty() {
        let Some((address, _)) = suggestion.as_ref() else {
            return Ok(None);
        };
        crate::commands::builder::parse_builder_address(address)?
    } else {
        crate::commands::builder::parse_builder_address(address)?
    };

    let fee_prompt = suggestion
        .as_ref()
        .map(|(_, fee)| format!("Default builder fee rate [{fee}]: "))
        .unwrap_or_else(|| "Default builder fee rate (for example 0.001%): ".to_string());
    let fee = prompt::read_line(&fee_prompt, "--yes")?;
    let fee = fee.trim();
    let fee = if fee.is_empty() {
        let Some((_, fee)) = suggestion.as_ref() else {
            return Err(CliError::Configuration(
                "default builder fee rate is required when a builder address is configured"
                    .to_string(),
            )
            .into());
        };
        fee.clone()
    } else {
        fee.to_string()
    };
    crate::commands::builder::validate_max_fee_rate(&fee)?;
    Ok(Some((parsed.to_string(), fee)))
}

pub(crate) fn setup_builder_default_suggestion() -> Result<Option<(String, String)>, anyhow::Error>
{
    crate::commands::builder::resolve_default_builder_fee_from_config(None)
        .map(|value| {
            value.map(|(address, fee)| {
                (
                    address.to_string(),
                    crate::commands::builder::percent_from_tenths_bps(fee),
                )
            })
        })
        .map_err(Into::into)
}

/// Decide, before anything changes, whether setup submits the builder approval.
fn resolve_builder_approval(
    args: &SetupArgs,
    builder_defaults: Option<&(String, String)>,
) -> Result<bool, anyhow::Error> {
    let Some((builder, fee)) = builder_defaults else {
        if args.approve_builder {
            return Err(CliError::Configuration(
                "--approve-builder requires a configured default builder address and fee rate"
                    .to_string(),
            )
            .into());
        }
        return Ok(false);
    };
    should_approve_default_builder(args, builder, fee)
}

async fn maybe_approve_default_builder(
    network: Network,
    approve: bool,
    builder_defaults: Option<&(String, String)>,
    wallet_id: &str,
    signer_address: Address,
    vault_path: Option<std::path::PathBuf>,
) -> Result<String, anyhow::Error> {
    let Some((builder, fee)) = builder_defaults else {
        return Ok("not configured".to_string());
    };
    if !approve {
        return Ok("not approved".to_string());
    }

    let builder_address = crate::commands::builder::parse_builder_address(builder)?;
    crate::commands::builder::validate_max_fee_rate(fee)?;
    let signer =
        crate::signing::SelectedSigner::ows(crate::ows::OwsSigningConfig::with_vault_path(
            wallet_id.to_string(),
            Some(wallet_id.to_string()),
            signer_address,
            vault_path,
        ));
    let chain = chain_for_network(network);
    let api_base_url = api_base_url_for_network(network)?;
    let approval = crate::commands::builder::submit_approval(
        &api_base_url,
        chain,
        &signer,
        builder_address,
        fee,
    )
    .await;
    match approval {
        Ok(max_fee_tenths_bps) => {
            let message =
                format!("approved {builder_address} at {fee} ({max_fee_tenths_bps} tenths bps)");
            eprintln!("Builder approval submitted: {message}");
            Ok(message)
        }
        Err(err) => {
            let message = format!("approval failed: {err}");
            eprintln!("Builder approval failed: {err}");
            Ok(message)
        }
    }
}

fn should_approve_default_builder(
    args: &SetupArgs,
    builder: &str,
    fee: &str,
) -> Result<bool, anyhow::Error> {
    if args.no_approve_builder {
        return Ok(false);
    }
    if args.yes || args.approve_builder {
        return Ok(true);
    }

    let answer = prompt::read_line(
        &format!("Approve default builder {builder} for max fee {fee}? [Y/n]: "),
        "--approve-builder",
    )?;
    match answer.trim().to_ascii_lowercase().as_str() {
        "" | "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        other => Err(CliError::Unsupported(format!(
            "builder approval answer '{other}' is not supported; answer yes or no"
        ))
        .into()),
    }
}

fn api_base_url_for_network(network: Network) -> Result<String, anyhow::Error> {
    Ok(config::resolve_api_base_url_override_for_network(network)?
        .map(|url| url.to_string())
        .unwrap_or_else(|| config::api_base_url(network == Network::Testnet).to_string()))
}

fn chain_for_network(network: Network) -> Chain {
    match network {
        Network::Mainnet => Chain::Mainnet,
        Network::Testnet => Chain::Testnet,
    }
}

fn prompt_referral_default(network: Network) -> Result<Option<String>, anyhow::Error> {
    let suggestion = setup_referral_default_suggestion(network)?;
    if let Some(code) = suggestion.as_ref() {
        eprintln!(
            "Default referral setup (optional; press Enter to use the suggested default, or type 'none' to skip):"
        );
        eprintln!("Suggested referral code: {code}");
    } else {
        eprintln!("Default referral setup (optional; press Enter to skip):");
    }

    let prompt_label = suggestion
        .as_ref()
        .map(|code| format!("Default referral code [{code}]: "))
        .unwrap_or_else(|| "Default referral code: ".to_string());
    let code = prompt::read_line(&prompt_label, "--yes")?;
    let code = code.trim();
    if code.eq_ignore_ascii_case("none") || code.eq_ignore_ascii_case("skip") {
        return Ok(None);
    }
    let code = if code.is_empty() {
        let Some(code) = suggestion else {
            return Ok(None);
        };
        code
    } else {
        code.to_string()
    };
    crate::commands::referral::validate_referral_code(&code)?;
    Ok(Some(code))
}

pub(crate) fn setup_referral_default_suggestion(
    network: Network,
) -> Result<Option<String>, anyhow::Error> {
    let code = crate::commands::referral::resolve_default_referral_code_from_config(
        network == Network::Testnet,
        None,
    );
    if let Some(code) = code.as_deref() {
        crate::commands::referral::validate_referral_code(code)?;
    }
    Ok(code)
}

fn next_setup_name(
    base: &str,
    vault_path: Option<&std::path::Path>,
) -> Result<String, anyhow::Error> {
    let existing =
        crate::ows::list_ows_wallets(vault_path).map_err(|err| anyhow::anyhow!("{err}"))?;
    let names: Vec<&str> = existing.iter().map(|w| w.name.as_str()).collect();
    if !names.contains(&base) {
        return Ok(base.to_string());
    }
    for index in 2.. {
        let candidate = format!("{base}-{index}");
        if !names.contains(&candidate.as_str()) {
            return Ok(candidate);
        }
    }
    unreachable!("unbounded name search should always return")
}

fn prompt_private_key() -> Result<String, CliError> {
    prompt::read_secret("Private key: ", "a private key on stdin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_summary_does_not_expose_private_key() {
        let summary = SetupSummary {
            message: "Setup complete".to_string(),
            address: "0xabc".to_string(),
            wallet_name: "setup".to_string(),
            network: "mainnet".to_string(),
            config_path: "config.json".to_string(),
            vault_path: "~/.hyperliquid".to_string(),
            connection: "Test query succeeded".to_string(),
            default_builder: "not configured".to_string(),
            builder_approval: "not configured".to_string(),
            default_referral_code: "not configured".to_string(),
            config_keys_written: vec!["network".to_string()],
        };

        let json = summary.to_json_value();
        assert!(json.get("private_key").is_none());
        assert!(crate::output::render(&summary, OutputFormat::Pretty).contains("Setup complete"));
    }

    #[test]
    fn setup_without_yes_fails_before_prompting_in_json_runs() {
        let args = SetupArgs {
            yes: false,
            approve_builder: false,
            no_approve_builder: false,
        };
        let err = ensure_setup_can_run(&args, OutputFormat::Json).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("hyperliquid setup --yes"));

        let yes = SetupArgs { yes: true, ..args };
        assert!(ensure_setup_can_run(&yes, OutputFormat::Json).is_ok());
    }
}
