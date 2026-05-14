//! Guided first-time setup wizard.

use std::io::{self, Write};

use clap::Args;
use hypersdk::Address;
use hypersdk::hypercore::{Chain, HttpClient};
use serde::Serialize;

use crate::commands::map_api_error;
use crate::config::{self, Config, Network};
use crate::output::{OutputFormat, TableData};

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
        ]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// Run the interactive first-time setup wizard.
pub async fn run(
    default_network: Network,
    args: &SetupArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    write_line("Welcome to Hyperliquid CLI setup", format)?;
    write_line(
        "This wizard will create or import a wallet, save your local config, and test the API connection.",
        format,
    )?;
    write_line("", format)?;
    write_line("Wallet setup:", format)?;
    write_line("  1) Create new wallet", format)?;
    write_line("  2) Import existing wallet (private key)", format)?;

    let wallet_choice = if args.yes {
        "1".to_string()
    } else {
        prompt("Choose an option [1/2]: ", format)?
    };
    let network = if args.yes {
        default_network
    } else {
        prompt_network(default_network, format)?
    };

    // Verify connection and collect fallible defaults before creating any local
    // wallet state so failures do not leave orphaned wallets behind.
    let connection = verify_connection(network).await?;
    write_line(&connection, format)?;
    let builder_defaults = if args.yes {
        setup_builder_default_suggestion()?
    } else {
        prompt_builder_defaults(format)?
    };
    let referral_code = if args.yes {
        setup_referral_default_suggestion(network)?
    } else {
        prompt_referral_default(network, format)?
    };

    let passphrase = crate::ows::ows_passphrase();
    let vault_path = crate::ows::ows_vault_path();

    let (wallet_name, wallet_id, address, signer_address) = match wallet_choice
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "1" | "create" | "c" => {
            let name = next_setup_name("setup", vault_path.as_deref())?;
            let wallet =
                crate::ows::create_ows_wallet(&name, passphrase.as_deref(), vault_path.as_deref())
                    .map_err(|err| anyhow::anyhow!("{err}"))?;
            let (hl, hl_address) = crate::ows::hyperliquid_address_from_wallet(&wallet)
                .map_err(|err| anyhow::anyhow!("{err}"))?;
            (wallet.name, wallet.id, hl, hl_address)
        }
        "2" | "import" | "i" => {
            let private_key = prompt_private_key(format)?;
            let name = next_setup_name("setup-imported", vault_path.as_deref())?;
            let wallet = crate::ows::import_ows_wallet_private_key(
                &name,
                private_key.trim(),
                passphrase.as_deref(),
                vault_path.as_deref(),
            )
            .map_err(|err| anyhow::anyhow!("{err}"))?;
            let (hl, hl_address) = crate::ows::hyperliquid_address_from_wallet(&wallet)
                .map_err(|err| anyhow::anyhow!("{err}"))?;
            (wallet.name, wallet.id, hl, hl_address)
        }
        other => {
            return Err(crate::errors::CliError::Unsupported(format!(
                "setup option '{other}' is not supported; choose 1 to create or 2 to import"
            ))
            .into());
        }
    };

    let config = Config {
        private_key: None,
        network,
        default_wallet_id: Some(wallet_id.clone()),
        default_builder_address: builder_defaults
            .as_ref()
            .map(|(address, _)| address.clone()),
        default_builder_fee_rate: builder_defaults.as_ref().map(|(_, fee)| fee.clone()),
        default_referral_code: referral_code.clone(),
    };
    config::save_config(&config).map_err(|err| {
        // Clean up the wallet we just created so the vault isn't left with
        // an orphaned wallet that no config references.
        let _ = crate::ows::delete_ows_wallet(&wallet_id, vault_path.as_deref());
        anyhow::anyhow!("failed to save config: {err}")
    })?;
    let config_path = config::config_file_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unavailable".to_string());
    write_line(&format!("Config saved to {config_path}"), format)?;

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
    let builder_approval = maybe_approve_default_builder(
        network,
        args,
        builder_defaults.as_ref(),
        &wallet_id,
        signer_address,
        vault_path.clone(),
        format,
    )
    .await?;

    let default_referral_code = referral_code.unwrap_or_else(|| "not configured".to_string());

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
    };
    crate::output::print_data_no_timing(&output, format);
    Ok(())
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

fn prompt_network(
    default_network: Network,
    format: OutputFormat,
) -> Result<Network, anyhow::Error> {
    let default_is_testnet = default_network == Network::Testnet;
    let default_hint = if default_is_testnet { "Y/n" } else { "y/N" };
    let answer = prompt(&format!("Use testnet? [{default_hint}]: "), format)?;
    match answer.trim().to_ascii_lowercase().as_str() {
        "" => Ok(default_network),
        "y" | "yes" => Ok(Network::Testnet),
        "n" | "no" => Ok(Network::Mainnet),
        other => Err(crate::errors::CliError::Unsupported(format!(
            "network answer '{other}' is not supported; answer yes or no"
        ))
        .into()),
    }
}

fn prompt_builder_defaults(
    format: OutputFormat,
) -> Result<Option<(String, String)>, anyhow::Error> {
    let suggestion = setup_builder_default_suggestion()?;
    if let Some((address, fee)) = suggestion.as_ref() {
        write_line(
            "Default builder fee setup (optional; press Enter to use the suggested default, or type 'none' to skip):",
            format,
        )?;
        write_line(&format!("Suggested builder: {address} at {fee}"), format)?;
    } else {
        write_line(
            "Default builder fee setup (optional; press Enter to skip):",
            format,
        )?;
    }

    let address_prompt = suggestion
        .as_ref()
        .map(|(address, _)| format!("Default builder address [{address}]: "))
        .unwrap_or_else(|| "Default builder address: ".to_string());
    let address = prompt(&address_prompt, format)?;
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
    let fee = prompt(&fee_prompt, format)?;
    let fee = fee.trim();
    let fee = if fee.is_empty() {
        let Some((_, fee)) = suggestion.as_ref() else {
            return Err(crate::errors::CliError::Configuration(
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

async fn maybe_approve_default_builder(
    network: Network,
    args: &SetupArgs,
    builder_defaults: Option<&(String, String)>,
    wallet_id: &str,
    signer_address: Address,
    vault_path: Option<std::path::PathBuf>,
    format: OutputFormat,
) -> Result<String, anyhow::Error> {
    let Some((builder, fee)) = builder_defaults else {
        if args.approve_builder {
            return Err(crate::errors::CliError::Configuration(
                "--approve-builder requires a configured default builder address and fee rate"
                    .to_string(),
            )
            .into());
        }
        return Ok("not configured".to_string());
    };

    let approve = should_approve_default_builder(args, builder, fee, format)?;
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
    let max_fee_tenths_bps = crate::commands::builder::submit_approval(
        &api_base_url,
        chain,
        &signer,
        builder_address,
        fee,
    )
    .await?;
    let message = format!("approved {builder_address} at {fee} ({max_fee_tenths_bps} tenths bps)");
    write_line(&format!("Builder approval submitted: {message}"), format)?;
    Ok(message)
}

fn should_approve_default_builder(
    args: &SetupArgs,
    builder: &str,
    fee: &str,
    format: OutputFormat,
) -> Result<bool, anyhow::Error> {
    if args.no_approve_builder {
        return Ok(false);
    }
    if args.yes || args.approve_builder {
        return Ok(true);
    }

    let answer = prompt(
        &format!("Approve default builder {builder} for max fee {fee}? [Y/n]: "),
        format,
    )?;
    match answer.trim().to_ascii_lowercase().as_str() {
        "" | "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        other => Err(crate::errors::CliError::Unsupported(format!(
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

fn prompt_referral_default(
    network: Network,
    format: OutputFormat,
) -> Result<Option<String>, anyhow::Error> {
    let suggestion = setup_referral_default_suggestion(network)?;
    if let Some(code) = suggestion.as_ref() {
        write_line(
            "Default referral setup (optional; press Enter to use the suggested default, or type 'none' to skip):",
            format,
        )?;
        write_line(&format!("Suggested referral code: {code}"), format)?;
    } else {
        write_line(
            "Default referral setup (optional; press Enter to skip):",
            format,
        )?;
    }

    let prompt_label = suggestion
        .as_ref()
        .map(|code| format!("Default referral code [{code}]: "))
        .unwrap_or_else(|| "Default referral code: ".to_string());
    let code = prompt(&prompt_label, format)?;
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

fn prompt_private_key(format: OutputFormat) -> Result<String, anyhow::Error> {
    use std::io::IsTerminal;

    if io::stdin().is_terminal() {
        return Ok(rpassword::prompt_password("Private key: ")?);
    }

    prompt("Private key: ", format)
}

fn prompt(label: &str, format: OutputFormat) -> Result<String, anyhow::Error> {
    write_prompt(label, format)?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim_end_matches(['\r', '\n']).to_string())
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
        };

        let json = summary.to_json_value();
        assert!(json.get("private_key").is_none());
        assert!(crate::output::render(&summary, OutputFormat::Pretty).contains("Setup complete"));
    }
}
