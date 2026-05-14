//! Public account data commands.
//!
//! Commands:
//! - `hyperliquid account fills <ADDRESS>` — public fill history
//! - `hyperliquid account orders <ADDRESS>` — public open orders
//! - `hyperliquid account portfolio <ADDRESS>` — public portfolio summary
//! - `hyperliquid account subaccounts <ADDRESS>` — public subaccount list

use std::io::{self, Write};
use std::time::{Duration, Instant};

use alloy::sol;
use chrono::DateTime;
use clap::{Args, Subcommand, ValueEnum};
use hypersdk::Address;
use hypersdk::Decimal;
use hypersdk::hypercore::{
    AssetPosition, BasicOrder, Chain, ClearinghouseState, Fill, HttpClient, SubAccount,
    UserVaultEquity,
};
use serde::Serialize;

use crate::command_context::CommandContext;
use crate::commands::spot_balances::{RawSpotBalance, user_spot_balances_raw};
use crate::commands::{actions, map_api_error, raw_info_base_url};
use crate::errors::CliError;
use crate::http_api::post_info_json;
use crate::output::{JsonValueOutput, OutputFormat, TableData, colors};
use crate::signing::SelectedSigner;

sol! {
    struct UserSetAbstraction {
        string hyperliquidChain;
        address user;
        string abstraction;
        uint64 nonce;
    }
}

#[derive(Args, Debug, Clone)]
pub struct FillsArgs {
    /// Ethereum address, stored account alias, or stored account id.
    /// Defaults to the selected/default signer when omitted.
    pub address: Option<String>,
    /// Start time as RFC3339 or epoch milliseconds. Uses userFillsByTime when present.
    #[arg(long, value_parser = parse_time_millis)]
    pub start: Option<u64>,
    /// End time as RFC3339 or epoch milliseconds.
    #[arg(long, requires = "start", value_parser = parse_time_millis)]
    pub end: Option<u64>,
    /// Combine partial fills from the same crossing order where supported by the API.
    #[arg(long, requires = "start")]
    pub aggregate_by_time: bool,
}

#[derive(Args, Debug, Clone)]
pub struct AddressArgs {
    /// Ethereum address, stored account alias, or stored account id.
    /// Defaults to the selected/default signer when omitted.
    pub address: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct TimeRangeArgs {
    /// Ethereum address, stored account alias, or stored account id.
    /// Defaults to the selected/default signer when omitted.
    pub address: Option<String>,
    /// Start time as RFC3339 or epoch milliseconds.
    #[arg(long, value_parser = parse_time_millis)]
    pub start: u64,
    /// End time as RFC3339 or epoch milliseconds.
    #[arg(long, value_parser = parse_time_millis)]
    pub end: Option<u64>,
}

#[derive(Args, Debug, Clone)]
pub struct TwapFillsArgs {
    /// Ethereum address, stored account alias, or stored account id.
    /// Defaults to the selected/default signer when omitted.
    pub address: Option<String>,
    /// Start time as RFC3339 or epoch milliseconds. Uses userTwapSliceFillsByTime when present.
    #[arg(long, value_parser = parse_time_millis)]
    pub start: Option<u64>,
    /// End time as RFC3339 or epoch milliseconds.
    #[arg(long, requires = "start", value_parser = parse_time_millis)]
    pub end: Option<u64>,
    /// Combine partial fills where supported by the API.
    #[arg(long, requires = "start")]
    pub aggregate_by_time: bool,
}

#[derive(Args, Debug, Clone)]
pub struct AbstractionArgs {
    /// Address, stored account alias, or stored account id to read
    pub address: Option<String>,

    #[command(subcommand)]
    pub command: Option<AbstractionCommand>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum AbstractionCommand {
    /// Set abstraction mode for the selected signer
    Set(SetAbstractionArgs),
}

#[derive(Args, Debug, Clone)]
pub struct SetAbstractionArgs {
    /// Abstraction mode to set
    #[arg(long, value_enum)]
    pub mode: AbstractionModeArg,

    /// Confirm live execution without prompting
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AbstractionModeArg {
    Disabled,
    UnifiedAccount,
    PortfolioMargin,
}

impl AbstractionModeArg {
    pub fn protocol_value(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::UnifiedAccount => "unifiedAccount",
            Self::PortfolioMargin => "portfolioMargin",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AbstractionRow {
    pub user: String,
    pub raw_mode: String,
    pub normalized_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbstractionOutput {
    pub row: AbstractionRow,
}

#[derive(Debug, Clone, Serialize)]
struct AbstractionSetRow {
    status: String,
    action: String,
    signer: String,
    user: String,
    mode: String,
    protocol_mode: String,
    network: String,
}

struct AbstractionSetOutput {
    row: AbstractionSetRow,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserSetAbstractionMessage {
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    user: Address,
    abstraction: String,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserSetAbstractionAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    user: Address,
    abstraction: String,
    nonce: u64,
}

pub fn parse_time_millis(raw: &str) -> Result<u64, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("time value cannot be empty".to_string());
    }

    if trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        return trimmed
            .parse::<u64>()
            .map_err(|err| format!("invalid epoch millisecond timestamp: {err}"));
    }

    let parsed = DateTime::parse_from_rfc3339(trimmed)
        .map_err(|err| format!("invalid RFC3339 or epoch millisecond timestamp: {err}"))?;
    let millis = parsed.timestamp_millis();
    if millis < 0 {
        return Err("time must be at or after the Unix epoch".to_string());
    }
    Ok(millis as u64)
}

impl TableData for AbstractionOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["User", "Mode", "Raw Mode"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.row.user.clone(),
            self.row.normalized_mode.clone(),
            self.row.raw_mode.clone(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.row).unwrap_or_else(|_| serde_json::json!({}))
    }
}

impl TableData for AbstractionSetOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Status", "Action", "Signer", "User", "Mode", "Network"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.row.status.clone(),
            self.row.action.clone(),
            self.row.signer.clone(),
            self.row.user.clone(),
            self.row.mode.clone(),
            self.row.network.clone(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.row).unwrap_or_else(|_| serde_json::json!({}))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccountQueryResult<T> {
    pub output: T,
    pub elapsed: Duration,
}

/// Fetch and render fill history for any public address.
pub async fn fills_query(
    client: &HttpClient,
    address: &str,
    args: &FillsArgs,
) -> Result<AccountQueryResult<FillsOutput>, anyhow::Error> {
    let start = Instant::now();
    let user = parse_address(address)?;
    validate_optional_time_range(args.start, args.end)?;
    let fills = if args.start.is_some() || args.aggregate_by_time {
        user_fills_raw(client, user, args.start, args.end, args.aggregate_by_time).await?
    } else {
        client
            .user_fills(user)
            .await
            .map_err(map_api_error)?
            .into_iter()
            .map(FillRow::from)
            .collect()
    };
    let output = FillsOutput::new(fills);

    Ok(AccountQueryResult {
        output,
        elapsed: start.elapsed(),
    })
}

pub async fn fills_with_context(
    context: &CommandContext<'_>,
    address: &str,
    args: &FillsArgs,
) -> Result<(), anyhow::Error> {
    let client = context.hypercore_client().ok_or_else(|| {
        anyhow::anyhow!("account fills command requires a Hyperliquid HTTP client")
    })?;
    let result = fills_query(client, address, args).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

/// Fetch and render fill history for any public address.
pub async fn fills(
    client: &HttpClient,
    address: &str,
    args: &FillsArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = fills_query(client, address, args).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Fetch and render open orders for any public address.
pub async fn orders_query(
    client: &HttpClient,
    address: &str,
) -> Result<AccountQueryResult<OrdersOutput>, anyhow::Error> {
    let start = Instant::now();
    let user = parse_address(address)?;
    let orders = client
        .open_orders(user, None)
        .await
        .map_err(map_api_error)?
        .into_iter()
        .map(OrderRow::from)
        .collect();
    let output = OrdersOutput::new(orders);

    Ok(AccountQueryResult {
        output,
        elapsed: start.elapsed(),
    })
}

pub async fn orders_with_context(
    context: &CommandContext<'_>,
    address: &str,
) -> Result<(), anyhow::Error> {
    let client = context.hypercore_client().ok_or_else(|| {
        anyhow::anyhow!("account orders command requires a Hyperliquid HTTP client")
    })?;
    let result = orders_query(client, address).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

/// Fetch and render open orders for any public address.
pub async fn orders(
    client: &HttpClient,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = orders_query(client, address).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Fetch and render a portfolio summary for any public address.
pub async fn portfolio_query(
    client: &HttpClient,
    address: &str,
) -> Result<AccountQueryResult<PortfolioOutput>, anyhow::Error> {
    let start = Instant::now();
    let user = parse_address(address)?;
    let state = client
        .clearinghouse_state(user, None)
        .await
        .map_err(map_api_error)?;
    let spot_balances = user_balances_raw(client, user).await?;
    let vault_equities = client
        .user_vault_equities(user)
        .await
        .map_err(map_api_error)?;
    let output = PortfolioOutput::new(state, spot_balances, vault_equities);

    Ok(AccountQueryResult {
        output,
        elapsed: start.elapsed(),
    })
}

pub async fn portfolio_with_context(
    context: &CommandContext<'_>,
    address: &str,
) -> Result<(), anyhow::Error> {
    let client = context.hypercore_client().ok_or_else(|| {
        anyhow::anyhow!("account portfolio command requires a Hyperliquid HTTP client")
    })?;
    let result = portfolio_query(client, address).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

/// Fetch and render a portfolio summary for any public address.
pub async fn portfolio(
    client: &HttpClient,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = portfolio_query(client, address).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Fetch and render subaccounts for any public master address.
pub async fn subaccounts_query(
    client: &HttpClient,
    address: &str,
) -> Result<AccountQueryResult<SubaccountsOutput>, anyhow::Error> {
    let start = Instant::now();
    let user = parse_address(address)?;
    let subaccounts = match client.subaccounts(user).await {
        Ok(subaccounts) => subaccounts,
        Err(err) if is_null_sequence_decode(&err) => Vec::new(),
        Err(err) => return Err(map_api_error(err).into()),
    }
    .into_iter()
    .map(SubaccountRow::from)
    .collect();
    let output = SubaccountsOutput::new(subaccounts);

    Ok(AccountQueryResult {
        output,
        elapsed: start.elapsed(),
    })
}

pub async fn subaccounts_with_context(
    context: &CommandContext<'_>,
    address: &str,
) -> Result<(), anyhow::Error> {
    let client = context.hypercore_client().ok_or_else(|| {
        anyhow::anyhow!("account subaccounts command requires a Hyperliquid HTTP client")
    })?;
    let result = subaccounts_query(client, address).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

/// Fetch and render subaccounts for any public master address.
pub async fn subaccounts(
    client: &HttpClient,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = subaccounts_query(client, address).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn fees(
    api_base_url: &str,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = fees_query(api_base_url, address).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn fees_query(
    api_base_url: &str,
    address: &str,
) -> Result<AccountQueryResult<JsonValueOutput>, anyhow::Error> {
    user_info_value_query(api_base_url, address, "userFees", "no fee data found").await
}

pub async fn fees_with_context(
    context: &CommandContext<'_>,
    address: &str,
) -> Result<(), anyhow::Error> {
    let result = fees_query(context.api_base_url(), address).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

pub async fn rate_limit(
    api_base_url: &str,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = rate_limit_query(api_base_url, address).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn rate_limit_query(
    api_base_url: &str,
    address: &str,
) -> Result<AccountQueryResult<JsonValueOutput>, anyhow::Error> {
    user_info_value_query(
        api_base_url,
        address,
        "userRateLimit",
        "no rate-limit data found",
    )
    .await
}

pub async fn rate_limit_with_context(
    context: &CommandContext<'_>,
    address: &str,
) -> Result<(), anyhow::Error> {
    let result = rate_limit_query(context.api_base_url(), address).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

pub async fn portfolio_history(
    api_base_url: &str,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = portfolio_history_query(api_base_url, address).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn portfolio_history_query(
    api_base_url: &str,
    address: &str,
) -> Result<AccountQueryResult<JsonValueOutput>, anyhow::Error> {
    user_info_value_query(
        api_base_url,
        address,
        "portfolio",
        "no portfolio history found",
    )
    .await
}

pub async fn portfolio_history_with_context(
    context: &CommandContext<'_>,
    address: &str,
) -> Result<(), anyhow::Error> {
    let result = portfolio_history_query(context.api_base_url(), address).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

pub async fn ledger(
    api_base_url: &str,
    address: &str,
    args: &TimeRangeArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = ledger_query(api_base_url, address, args).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn ledger_query(
    api_base_url: &str,
    address: &str,
    args: &TimeRangeArgs,
) -> Result<AccountQueryResult<JsonValueOutput>, anyhow::Error> {
    user_time_range_info_value_query(
        api_base_url,
        address,
        "userNonFundingLedgerUpdates",
        args.start,
        args.end,
        "no ledger updates found",
    )
    .await
}

pub async fn ledger_with_context(
    context: &CommandContext<'_>,
    address: &str,
    args: &TimeRangeArgs,
) -> Result<(), anyhow::Error> {
    let result = ledger_query(context.api_base_url(), address, args).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

pub async fn funding(
    api_base_url: &str,
    address: &str,
    args: &TimeRangeArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = funding_query(api_base_url, address, args).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn funding_query(
    api_base_url: &str,
    address: &str,
    args: &TimeRangeArgs,
) -> Result<AccountQueryResult<JsonValueOutput>, anyhow::Error> {
    user_time_range_info_value_query(
        api_base_url,
        address,
        "userFunding",
        args.start,
        args.end,
        "no funding history found",
    )
    .await
}

pub async fn funding_with_context(
    context: &CommandContext<'_>,
    address: &str,
    args: &TimeRangeArgs,
) -> Result<(), anyhow::Error> {
    let result = funding_query(context.api_base_url(), address, args).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

pub async fn twap_history(
    api_base_url: &str,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = twap_history_query(api_base_url, address).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn twap_history_query(
    api_base_url: &str,
    address: &str,
) -> Result<AccountQueryResult<JsonValueOutput>, anyhow::Error> {
    user_info_value_query(
        api_base_url,
        address,
        "twapHistory",
        "no TWAP history found",
    )
    .await
}

pub async fn twap_history_with_context(
    context: &CommandContext<'_>,
    address: &str,
) -> Result<(), anyhow::Error> {
    let result = twap_history_query(context.api_base_url(), address).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

pub async fn twap_fills_query(
    api_base_url: &str,
    address: &str,
    args: &TwapFillsArgs,
) -> Result<AccountQueryResult<JsonValueOutput>, anyhow::Error> {
    let start = Instant::now();
    let user = parse_address(address)?;
    validate_optional_time_range(args.start, args.end)?;
    let value = if let Some(start_time) = args.start {
        let request = UserTwapSliceFillsByTimeRequest {
            request_type: "userTwapSliceFillsByTime",
            user,
            start_time,
            end_time: args.end,
            aggregate_by_time: args.aggregate_by_time.then_some(true),
        };
        post_info_json::<serde_json::Value>(
            api_base_url,
            &request,
            "loading TWAP slice fills by time",
        )
        .await?
    } else {
        let request = UserRequest {
            request_type: "userTwapSliceFills",
            user,
        };
        post_info_json::<serde_json::Value>(api_base_url, &request, "loading TWAP slice fills")
            .await?
    };

    let output = JsonValueOutput::new(value, "no TWAP slice fills found");
    Ok(AccountQueryResult {
        output,
        elapsed: start.elapsed(),
    })
}

pub async fn twap_fills_with_context(
    context: &CommandContext<'_>,
    address: &str,
    args: &TwapFillsArgs,
) -> Result<(), anyhow::Error> {
    let result = twap_fills_query(context.api_base_url(), address, args).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

pub async fn twap_fills(
    api_base_url: &str,
    address: &str,
    args: &TwapFillsArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = twap_fills_query(api_base_url, address, args).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn abstraction_query(
    api_base_url: &str,
    address: &str,
) -> Result<AccountQueryResult<AbstractionOutput>, anyhow::Error> {
    let start = Instant::now();
    let user = parse_address(address)?;
    let request = UserRequest {
        request_type: "userAbstraction",
        user,
    };
    let raw_mode =
        post_info_json::<String>(api_base_url, &request, "loading user abstraction").await?;
    let row = AbstractionRow {
        user: user.to_string(),
        normalized_mode: normalize_abstraction_mode(&raw_mode).to_string(),
        raw_mode,
    };
    Ok(AccountQueryResult {
        output: AbstractionOutput { row },
        elapsed: start.elapsed(),
    })
}

pub async fn abstraction_with_context(
    context: &CommandContext<'_>,
    address: &str,
) -> Result<(), anyhow::Error> {
    let result = abstraction_query(context.api_base_url(), address).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

pub async fn abstraction(
    api_base_url: &str,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = abstraction_query(api_base_url, address).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub fn abstraction_set_dry_run_value(
    chain: Chain,
    signer: Address,
    args: &SetAbstractionArgs,
) -> serde_json::Value {
    serde_json::json!({
        "mode": mode_cli_value(args.mode),
        "protocol_mode": args.mode.protocol_value(),
        "network": chain.to_string(),
        "user": signer.to_string(),
        "action": {
            "type": "userSetAbstraction",
            "hyperliquidChain": chain.to_string(),
            "signatureChainId": chain.arbitrum_id().to_string(),
            "user": signer.to_string(),
            "abstraction": args.mode.protocol_value(),
        }
    })
}

pub async fn set_abstraction(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    args: &SetAbstractionArgs,
    require_confirmation: bool,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    if require_confirmation && !args.yes {
        confirm_account_mode_change(args.mode, signer.address(), format)?;
    }
    let start = Instant::now();
    let user = signer.address();
    let nonce = actions::nonce_now();
    let protocol_mode = args.mode.protocol_value().to_string();
    let message = UserSetAbstractionMessage {
        signature_chain_id: chain.arbitrum_id().to_string(),
        hyperliquid_chain: chain,
        user,
        abstraction: protocol_mode.clone(),
        nonce,
    };
    let signature = actions::sign_user_action::<UserSetAbstraction>(signer, chain, &message)?;
    let action = UserSetAbstractionAction {
        action_type: "userSetAbstraction",
        signature_chain_id: chain.arbitrum_id().to_string(),
        hyperliquid_chain: chain,
        user,
        abstraction: protocol_mode.clone(),
        nonce,
    };
    actions::send_user_signed_json_action(
        api_base_url,
        serde_json::to_value(action).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?,
        nonce,
        signature,
    )
    .await?;

    crate::output::print_data(
        &AbstractionSetOutput {
            row: AbstractionSetRow {
                status: "submitted".to_string(),
                action: "set-abstraction".to_string(),
                signer: user.to_string(),
                user: user.to_string(),
                mode: mode_cli_value(args.mode).to_string(),
                protocol_mode,
                network: chain.to_string(),
            },
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

fn parse_address(address: &str) -> Result<Address, CliError> {
    address
        .parse::<Address>()
        .map_err(|_| CliError::Unsupported(format!("Invalid address: {address}")))
}

fn normalize_abstraction_mode(raw: &str) -> &'static str {
    match raw {
        "unifiedAccount" => "unified-account",
        "portfolioMargin" => "portfolio-margin",
        "disabled" => "disabled",
        "default" => "default",
        "dexAbstraction" => "dex-abstraction",
        _ => "unknown",
    }
}

fn mode_cli_value(mode: AbstractionModeArg) -> &'static str {
    match mode {
        AbstractionModeArg::Disabled => "disabled",
        AbstractionModeArg::UnifiedAccount => "unified-account",
        AbstractionModeArg::PortfolioMargin => "portfolio-margin",
    }
}

fn confirm_account_mode_change(
    mode: AbstractionModeArg,
    user: Address,
    format: OutputFormat,
) -> Result<(), CliError> {
    let prompt = format!(
        "Set account abstraction for {user} to {}? [y/N] ",
        mode_cli_value(mode)
    );
    let prompt = if format == OutputFormat::Pretty {
        colors::yellow(&prompt)
    } else {
        prompt
    };
    let mut stderr = io::stderr();
    write!(stderr, "{prompt}").map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    stderr
        .flush()
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    if matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        Err(CliError::Configuration(
            "account abstraction update aborted".to_string(),
        ))
    }
}

fn validate_optional_time_range(start: Option<u64>, end: Option<u64>) -> Result<(), CliError> {
    if let (Some(start), Some(end)) = (start, end)
        && end < start
    {
        return Err(CliError::Configuration(
            "--end must be greater than or equal to --start".to_string(),
        ));
    }
    Ok(())
}

fn validate_time_range(start: u64, end: Option<u64>) -> Result<(), CliError> {
    validate_optional_time_range(Some(start), end)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserRequest {
    #[serde(rename = "type")]
    request_type: &'static str,
    user: Address,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserFillsRequest {
    #[serde(rename = "type")]
    request_type: &'static str,
    user: Address,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aggregate_by_time: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserTimeRangeRequest {
    #[serde(rename = "type")]
    request_type: &'static str,
    user: Address,
    start_time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_time: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserTwapSliceFillsByTimeRequest {
    #[serde(rename = "type")]
    request_type: &'static str,
    user: Address,
    start_time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aggregate_by_time: Option<bool>,
}

async fn user_balances_raw(
    client: &HttpClient,
    user: Address,
) -> Result<Vec<BalanceRow>, CliError> {
    Ok(user_spot_balances_raw(client, user)
        .await?
        .into_iter()
        .map(BalanceRow::from)
        .collect())
}

async fn user_fills_raw(
    client: &HttpClient,
    user: Address,
    start_time: Option<u64>,
    end_time: Option<u64>,
    aggregate_by_time: bool,
) -> Result<Vec<FillRow>, CliError> {
    let request = UserFillsRequest {
        request_type: if start_time.is_some() {
            "userFillsByTime"
        } else {
            "userFills"
        },
        user,
        start_time,
        end_time,
        aggregate_by_time: aggregate_by_time.then_some(true),
    };
    let api_url = raw_info_base_url(client)?;
    let context = if start_time.is_some() {
        "loading user fills by time"
    } else {
        "loading user fills"
    };
    let fills = post_info_json::<Vec<Fill>>(api_url.as_str(), &request, context).await?;
    Ok(fills.into_iter().map(FillRow::from).collect())
}

async fn user_info_value_query(
    api_base_url: &str,
    address: &str,
    request_type: &'static str,
    empty_message: &'static str,
) -> Result<AccountQueryResult<JsonValueOutput>, anyhow::Error> {
    let start = Instant::now();
    let user = parse_address(address)?;
    let request = UserRequest { request_type, user };
    let value = post_info_json::<serde_json::Value>(api_base_url, &request, request_type).await?;
    let output = JsonValueOutput::new(value, empty_message);
    Ok(AccountQueryResult {
        output,
        elapsed: start.elapsed(),
    })
}

async fn user_time_range_info_value_query(
    api_base_url: &str,
    address: &str,
    request_type: &'static str,
    start_time: u64,
    end_time: Option<u64>,
    empty_message: &'static str,
) -> Result<AccountQueryResult<JsonValueOutput>, anyhow::Error> {
    let start = Instant::now();
    let user = parse_address(address)?;
    validate_time_range(start_time, end_time)?;
    let request = UserTimeRangeRequest {
        request_type,
        user,
        start_time,
        end_time,
    };
    let value = post_info_json::<serde_json::Value>(api_base_url, &request, request_type).await?;
    let output = JsonValueOutput::new(value, empty_message);
    Ok(AccountQueryResult {
        output,
        elapsed: start.elapsed(),
    })
}

fn is_null_sequence_decode(err: &anyhow::Error) -> bool {
    let message = err.to_string();
    message.contains("invalid type: null")
        && message.contains("expected a sequence")
        && message.contains("body=null")
}

fn format_pretty_pnl(value: Decimal) -> String {
    let formatted = value.to_string();
    if value > Decimal::ZERO {
        colors::green(&formatted)
    } else if value < Decimal::ZERO {
        colors::red(&formatted)
    } else {
        formatted
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FillRow {
    pub coin: String,
    pub side: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub price: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub size: Decimal,
    pub direction: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub closed_pnl: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub fee: Decimal,
    pub fee_token: String,
    pub oid: u64,
    pub time: u64,
    pub liquidity: String,
    pub trade_id: u64,
    pub cloid: Option<String>,
}

impl From<Fill> for FillRow {
    fn from(fill: Fill) -> Self {
        Self {
            coin: fill.coin,
            side: fill.side.to_string(),
            price: fill.px,
            size: fill.sz,
            direction: fill.dir,
            closed_pnl: fill.closed_pnl,
            fee: fill.fee,
            fee_token: fill.fee_token,
            oid: fill.oid,
            time: fill.time,
            liquidity: if fill.crossed { "taker" } else { "maker" }.to_string(),
            trade_id: fill.tid,
            cloid: fill.cloid.map(|cloid| cloid.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillsOutput {
    fills: Vec<FillRow>,
}

impl FillsOutput {
    #[must_use]
    pub fn new(fills: Vec<FillRow>) -> Self {
        Self { fills }
    }
}

impl TableData for FillsOutput {
    fn headers(&self) -> Vec<&str> {
        if self.fills.is_empty() {
            vec!["Message"]
        } else {
            vec![
                "Coin",
                "Side",
                "Price",
                "Size",
                "Direction",
                "Closed PnL",
                "Fee",
                "Fee Token",
                "OID",
                "Time",
                "Liquidity",
            ]
        }
    }

    fn rows(&self) -> Vec<Vec<String>> {
        if self.fills.is_empty() {
            return vec![vec!["no fills found".to_string()]];
        }

        self.fills
            .iter()
            .map(|fill| {
                vec![
                    fill.coin.clone(),
                    fill.side.clone(),
                    fill.price.to_string(),
                    fill.size.to_string(),
                    fill.direction.clone(),
                    fill.closed_pnl.to_string(),
                    fill.fee.to_string(),
                    fill.fee_token.clone(),
                    fill.oid.to_string(),
                    fill.time.to_string(),
                    fill.liquidity.clone(),
                ]
            })
            .collect()
    }

    fn pretty_rows(&self) -> Vec<Vec<String>> {
        if self.fills.is_empty() {
            return self.rows();
        }

        self.fills
            .iter()
            .map(|fill| {
                vec![
                    fill.coin.clone(),
                    fill.side.clone(),
                    fill.price.to_string(),
                    fill.size.to_string(),
                    fill.direction.clone(),
                    format_pretty_pnl(fill.closed_pnl),
                    fill.fee.to_string(),
                    fill.fee_token.clone(),
                    fill.oid.to_string(),
                    fill.time.to_string(),
                    fill.liquidity.clone(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.fills).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OrderRow {
    pub coin: String,
    pub side: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub limit_price: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub size: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub original_size: Decimal,
    pub oid: u64,
    pub order_type: String,
    pub tif: Option<String>,
    pub reduce_only: bool,
    pub timestamp: u64,
    pub cloid: Option<String>,
}

impl From<BasicOrder> for OrderRow {
    fn from(order: BasicOrder) -> Self {
        Self {
            coin: order.coin,
            side: order.side.to_string(),
            limit_price: order.limit_px,
            size: order.sz,
            original_size: order.orig_sz,
            oid: order.oid,
            order_type: format!("{:?}", order.order_type),
            tif: order.tif.map(|tif| format!("{tif:?}")),
            reduce_only: order.reduce_only,
            timestamp: order.timestamp,
            cloid: order.cloid.map(|cloid| cloid.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrdersOutput {
    orders: Vec<OrderRow>,
}

impl OrdersOutput {
    #[must_use]
    pub fn new(orders: Vec<OrderRow>) -> Self {
        Self { orders }
    }
}

impl TableData for OrdersOutput {
    fn headers(&self) -> Vec<&str> {
        if self.orders.is_empty() {
            vec!["Message"]
        } else {
            vec![
                "Coin",
                "Side",
                "Limit Price",
                "Size",
                "Original Size",
                "OID",
                "Type",
                "TIF",
                "Reduce Only",
                "Timestamp",
            ]
        }
    }

    fn rows(&self) -> Vec<Vec<String>> {
        if self.orders.is_empty() {
            return vec![vec!["no orders found".to_string()]];
        }

        self.orders
            .iter()
            .map(|order| {
                vec![
                    order.coin.clone(),
                    order.side.clone(),
                    order.limit_price.to_string(),
                    order.size.to_string(),
                    order.original_size.to_string(),
                    order.oid.to_string(),
                    order.order_type.clone(),
                    order.tif.clone().unwrap_or_else(|| "n/a".to_string()),
                    order.reduce_only.to_string(),
                    order.timestamp.to_string(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.orders).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PortfolioSummaryRow {
    #[serde(with = "rust_decimal::serde::str")]
    pub account_value: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub total_notional_position: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub total_raw_usd: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub total_margin_used: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub cross_account_value: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub withdrawable: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub margin_utilization_pct: Decimal,
    pub positions_count: usize,
    pub spot_balances_count: usize,
    pub vault_equities_count: usize,
    pub time: u64,
}

impl From<&ClearinghouseState> for PortfolioSummaryRow {
    fn from(state: &ClearinghouseState) -> Self {
        Self {
            account_value: state.margin_summary.account_value,
            total_notional_position: state.margin_summary.total_ntl_pos,
            total_raw_usd: state.margin_summary.total_raw_usd,
            total_margin_used: state.margin_summary.total_margin_used,
            cross_account_value: state.cross_margin_summary.account_value,
            withdrawable: state.withdrawable,
            margin_utilization_pct: state.margin_summary.margin_utilization(),
            positions_count: state.asset_positions.len(),
            spot_balances_count: 0,
            vault_equities_count: 0,
            time: state.time,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PositionRow {
    pub coin: String,
    pub side: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub size: Decimal,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub entry_price: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str")]
    pub position_value: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub unrealized_pnl: Decimal,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub liquidation_price: Option<Decimal>,
    pub leverage: String,
}

impl From<AssetPosition> for PositionRow {
    fn from(asset_position: AssetPosition) -> Self {
        let position = asset_position.position;
        let side = position.side().to_string();
        Self {
            coin: position.coin,
            side,
            size: position.szi,
            entry_price: position.entry_px,
            position_value: position.position_value,
            unrealized_pnl: position.unrealized_pnl,
            liquidation_price: position.liquidation_px,
            leverage: format!(
                "{}x {}",
                position.leverage.value, position.leverage.leverage_type
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BalanceRow {
    pub coin: String,
    pub token: Option<usize>,
    #[serde(with = "rust_decimal::serde::str")]
    pub hold: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub total: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub entry_notional: Decimal,
}

impl From<RawSpotBalance> for BalanceRow {
    fn from(balance: RawSpotBalance) -> Self {
        Self {
            coin: balance.coin,
            token: balance.token,
            hold: balance.hold,
            total: balance.total,
            entry_notional: balance.entry_ntl,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VaultEquityRow {
    pub vault_address: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub equity: Decimal,
    pub locked_until_timestamp: Option<u64>,
}

impl From<UserVaultEquity> for VaultEquityRow {
    fn from(equity: UserVaultEquity) -> Self {
        Self {
            vault_address: equity.vault_address.to_string(),
            equity: equity.equity,
            locked_until_timestamp: equity.locked_until_timestamp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioOutput {
    summary: PortfolioSummaryRow,
    positions: Vec<PositionRow>,
    spot_balances: Vec<BalanceRow>,
    vault_equities: Vec<VaultEquityRow>,
}

impl PortfolioOutput {
    #[must_use]
    pub fn new(
        state: ClearinghouseState,
        spot_balances: Vec<BalanceRow>,
        vault_equities: Vec<UserVaultEquity>,
    ) -> Self {
        let mut summary = PortfolioSummaryRow::from(&state);
        let positions = state
            .asset_positions
            .into_iter()
            .map(PositionRow::from)
            .collect::<Vec<_>>();
        let vault_equities = vault_equities
            .into_iter()
            .map(VaultEquityRow::from)
            .collect::<Vec<_>>();
        summary.positions_count = positions.len();
        summary.spot_balances_count = spot_balances.len();
        summary.vault_equities_count = vault_equities.len();

        Self {
            summary,
            positions,
            spot_balances,
            vault_equities,
        }
    }
}

impl TableData for PortfolioOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Section", "Name", "Value", "Details"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        let mut rows = vec![
            vec![
                "Summary".to_string(),
                "Account Value".to_string(),
                self.summary.account_value.to_string(),
                "Total equity".to_string(),
            ],
            vec![
                "Summary".to_string(),
                "Withdrawable".to_string(),
                self.summary.withdrawable.to_string(),
                "Available to withdraw".to_string(),
            ],
            vec![
                "Summary".to_string(),
                "Margin Used".to_string(),
                self.summary.total_margin_used.to_string(),
                format!("{}% utilization", self.summary.margin_utilization_pct),
            ],
        ];

        if self.positions.is_empty()
            && self.spot_balances.is_empty()
            && self.vault_equities.is_empty()
        {
            rows.push(vec![
                "Portfolio".to_string(),
                "Holdings".to_string(),
                "no data found".to_string(),
                "No positions, spot balances, or vault equities returned".to_string(),
            ]);
        }

        rows.extend(self.positions.iter().map(|position| {
            vec![
                "Position".to_string(),
                position.coin.clone(),
                position.size.to_string(),
                format!(
                    "{} pnl={} value={}",
                    position.side, position.unrealized_pnl, position.position_value
                ),
            ]
        }));
        rows.extend(self.spot_balances.iter().map(|balance| {
            vec![
                "Spot Balance".to_string(),
                balance.coin.clone(),
                balance.total.to_string(),
                format!(
                    "hold={} token={}",
                    balance.hold,
                    balance
                        .token
                        .map(|token| token.to_string())
                        .unwrap_or_else(|| "n/a".to_string())
                ),
            ]
        }));
        rows.extend(self.vault_equities.iter().map(|equity| {
            vec![
                "Vault Equity".to_string(),
                equity.vault_address.clone(),
                equity.equity.to_string(),
                equity
                    .locked_until_timestamp
                    .map(|ts| format!("locked_until={ts}"))
                    .unwrap_or_else(|| "unlocked".to_string()),
            ]
        }));

        rows
    }

    fn pretty_rows(&self) -> Vec<Vec<String>> {
        let mut rows = vec![
            vec![
                "Summary".to_string(),
                "Account Value".to_string(),
                self.summary.account_value.to_string(),
                "Total equity".to_string(),
            ],
            vec![
                "Summary".to_string(),
                "Withdrawable".to_string(),
                self.summary.withdrawable.to_string(),
                "Available to withdraw".to_string(),
            ],
            vec![
                "Summary".to_string(),
                "Margin Used".to_string(),
                self.summary.total_margin_used.to_string(),
                format!("{}% utilization", self.summary.margin_utilization_pct),
            ],
        ];

        if self.positions.is_empty()
            && self.spot_balances.is_empty()
            && self.vault_equities.is_empty()
        {
            rows.push(vec![
                "Portfolio".to_string(),
                "Holdings".to_string(),
                "no data found".to_string(),
                "No positions, spot balances, or vault equities returned".to_string(),
            ]);
        }

        rows.extend(self.positions.iter().map(|position| {
            vec![
                "Position".to_string(),
                position.coin.clone(),
                position.size.to_string(),
                format!(
                    "{} pnl={} value={}",
                    position.side,
                    format_pretty_pnl(position.unrealized_pnl),
                    position.position_value
                ),
            ]
        }));
        rows.extend(self.spot_balances.iter().map(|balance| {
            vec![
                "Spot Balance".to_string(),
                balance.coin.clone(),
                balance.total.to_string(),
                format!(
                    "hold={} token={}",
                    balance.hold,
                    balance
                        .token
                        .map(|token| token.to_string())
                        .unwrap_or_else(|| "n/a".to_string())
                ),
            ]
        }));
        rows.extend(self.vault_equities.iter().map(|equity| {
            vec![
                "Vault Equity".to_string(),
                equity.vault_address.clone(),
                equity.equity.to_string(),
                equity
                    .locked_until_timestamp
                    .map(|ts| format!("locked_until={ts}"))
                    .unwrap_or_else(|| "unlocked".to_string()),
            ]
        }));

        rows
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "summary": self.summary,
            "positions": self.positions,
            "spot_balances": self.spot_balances,
            "vault_equities": self.vault_equities,
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SubaccountRow {
    pub name: String,
    pub address: String,
    pub master: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub account_value: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub withdrawable: Decimal,
    pub positions_count: usize,
    pub spot_balances_count: usize,
}

impl From<SubAccount> for SubaccountRow {
    fn from(subaccount: SubAccount) -> Self {
        Self {
            name: subaccount.name,
            address: subaccount.sub_account_user.to_string(),
            master: subaccount.master.to_string(),
            account_value: subaccount.clearinghouse_state.margin_summary.account_value,
            withdrawable: subaccount.clearinghouse_state.withdrawable,
            positions_count: subaccount.clearinghouse_state.asset_positions.len(),
            spot_balances_count: subaccount.spot_state.balances.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubaccountsOutput {
    subaccounts: Vec<SubaccountRow>,
}

impl SubaccountsOutput {
    #[must_use]
    pub fn new(subaccounts: Vec<SubaccountRow>) -> Self {
        Self { subaccounts }
    }
}

impl TableData for SubaccountsOutput {
    fn headers(&self) -> Vec<&str> {
        if self.subaccounts.is_empty() {
            vec!["Message"]
        } else {
            vec![
                "Name",
                "Address",
                "Master",
                "Account Value",
                "Withdrawable",
                "Positions",
                "Spot Balances",
            ]
        }
    }

    fn rows(&self) -> Vec<Vec<String>> {
        if self.subaccounts.is_empty() {
            return vec![vec!["no subaccounts found".to_string()]];
        }

        self.subaccounts
            .iter()
            .map(|subaccount| {
                vec![
                    subaccount.name.clone(),
                    subaccount.address.clone(),
                    subaccount.master.clone(),
                    subaccount.account_value.to_string(),
                    subaccount.withdrawable.to_string(),
                    subaccount.positions_count.to_string(),
                    subaccount.spot_balances_count.to_string(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.subaccounts).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(input: &str) -> Decimal {
        Decimal::from_str(input).unwrap()
    }

    #[test]
    fn empty_fills_show_no_data_message_but_json_is_empty_array() {
        let output = FillsOutput::new(vec![]);

        assert_eq!(output.rows()[0][0], "no fills found");
        assert!(output.to_json_value().as_array().unwrap().is_empty());
    }

    #[test]
    fn empty_orders_show_no_data_message_but_json_is_empty_array() {
        let output = OrdersOutput::new(vec![]);

        assert_eq!(output.rows()[0][0], "no orders found");
        assert!(output.to_json_value().as_array().unwrap().is_empty());
    }

    #[test]
    fn empty_subaccounts_show_no_data_message_but_json_is_empty_array() {
        let output = SubaccountsOutput::new(vec![]);

        assert_eq!(output.rows()[0][0], "no subaccounts found");
        assert!(output.to_json_value().as_array().unwrap().is_empty());
    }

    #[test]
    fn fill_json_has_public_account_fields() {
        let output = FillsOutput::new(vec![FillRow {
            coin: "BTC".to_string(),
            side: "Bid".to_string(),
            price: dec("100"),
            size: dec("0.5"),
            direction: "Open Long".to_string(),
            closed_pnl: Decimal::ZERO,
            fee: dec("0.01"),
            fee_token: "USDC".to_string(),
            oid: 42,
            time: 123,
            liquidity: "taker".to_string(),
            trade_id: 7,
            cloid: None,
        }]);
        let json = output.to_json_value();

        assert_eq!(json[0]["coin"], "BTC");
        assert_eq!(json[0]["price"], "100");
        assert_eq!(json[0]["fee_token"], "USDC");
    }

    #[test]
    fn fills_pretty_colors_positive_and_negative_closed_pnl_cells() {
        let output = FillsOutput::new(vec![
            FillRow {
                coin: "BTC".to_string(),
                side: "Bid".to_string(),
                price: dec("100"),
                size: dec("0.5"),
                direction: "Close Long".to_string(),
                closed_pnl: dec("12.34"),
                fee: dec("0.01"),
                fee_token: "USDC".to_string(),
                oid: 42,
                time: 123,
                liquidity: "taker".to_string(),
                trade_id: 7,
                cloid: None,
            },
            FillRow {
                coin: "ETH".to_string(),
                side: "Ask".to_string(),
                price: dec("90"),
                size: dec("1"),
                direction: "Close Short".to_string(),
                closed_pnl: dec("-5.67"),
                fee: dec("0.02"),
                fee_token: "USDC".to_string(),
                oid: 43,
                time: 124,
                liquidity: "maker".to_string(),
                trade_id: 8,
                cloid: None,
            },
        ]);

        let pretty = crate::output::render(&output, OutputFormat::Pretty);

        assert!(pretty.contains(&crate::output::colors::green("12.34")));
        assert!(pretty.contains(&crate::output::colors::red("-5.67")));
        assert!(pretty.contains(&crate::output::colors::cyan("Closed PnL")));
    }

    #[test]
    fn fills_table_and_json_keep_closed_pnl_uncolored() {
        let output = FillsOutput::new(vec![FillRow {
            coin: "BTC".to_string(),
            side: "Bid".to_string(),
            price: dec("100"),
            size: dec("0.5"),
            direction: "Close Long".to_string(),
            closed_pnl: dec("12.34"),
            fee: dec("0.01"),
            fee_token: "USDC".to_string(),
            oid: 42,
            time: 123,
            liquidity: "taker".to_string(),
            trade_id: 7,
            cloid: None,
        }]);

        let table = crate::output::render(&output, OutputFormat::Table);
        let json = crate::output::render(&output, OutputFormat::Json);

        assert!(table.contains("12.34"));
        assert!(!table.contains(crate::output::colors::GREEN));
        assert!(!table.contains(crate::output::colors::RED));
        assert!(json.contains("\"closed_pnl\": \"12.34\""));
        assert!(!json.contains(crate::output::colors::GREEN));
        assert!(!json.contains(crate::output::colors::RED));
    }

    #[test]
    fn zero_closed_pnl_is_not_forced_red_or_green() {
        let output = FillsOutput::new(vec![FillRow {
            coin: "BTC".to_string(),
            side: "Bid".to_string(),
            price: dec("100"),
            size: dec("0.5"),
            direction: "Close Long".to_string(),
            closed_pnl: Decimal::ZERO,
            fee: dec("0.01"),
            fee_token: "USDC".to_string(),
            oid: 42,
            time: 123,
            liquidity: "taker".to_string(),
            trade_id: 7,
            cloid: None,
        }]);

        let pretty = crate::output::render(&output, OutputFormat::Pretty);

        assert!(pretty.contains("\t0\t") || pretty.contains(" 0 "));
        assert!(!pretty.contains(&crate::output::colors::green("0")));
        assert!(!pretty.contains(&crate::output::colors::red("0")));
    }

    #[test]
    fn portfolio_pretty_colors_position_unrealized_pnl_only() {
        let output = PortfolioOutput {
            summary: PortfolioSummaryRow {
                account_value: dec("1000"),
                total_notional_position: dec("100"),
                total_raw_usd: dec("1000"),
                total_margin_used: dec("10"),
                cross_account_value: dec("1000"),
                withdrawable: dec("990"),
                margin_utilization_pct: dec("1"),
                positions_count: 2,
                spot_balances_count: 0,
                vault_equities_count: 0,
                time: 123,
            },
            positions: vec![
                PositionRow {
                    coin: "BTC".to_string(),
                    side: "long".to_string(),
                    size: dec("0.1"),
                    entry_price: Some(dec("100")),
                    position_value: dec("110"),
                    unrealized_pnl: dec("10"),
                    liquidation_price: None,
                    leverage: "1x cross".to_string(),
                },
                PositionRow {
                    coin: "ETH".to_string(),
                    side: "short".to_string(),
                    size: dec("-1"),
                    entry_price: Some(dec("100")),
                    position_value: dec("95"),
                    unrealized_pnl: dec("-5"),
                    liquidation_price: None,
                    leverage: "1x cross".to_string(),
                },
            ],
            spot_balances: vec![BalanceRow {
                coin: "USDC".to_string(),
                token: Some(0),
                hold: Decimal::ZERO,
                total: dec("100"),
                entry_notional: Decimal::ZERO,
            }],
            vault_equities: vec![],
        };

        let pretty = crate::output::render(&output, OutputFormat::Pretty);
        let table = crate::output::render(&output, OutputFormat::Table);

        assert!(pretty.contains(&format!("pnl={}", crate::output::colors::green("10"))));
        assert!(pretty.contains(&format!("pnl={}", crate::output::colors::red("-5"))));
        assert!(!pretty.contains(&crate::output::colors::green("100")));
        assert!(table.contains("pnl=10"));
        assert!(table.contains("pnl=-5"));
        assert!(!table.contains(crate::output::colors::GREEN));
        assert!(!table.contains(crate::output::colors::RED));
    }

    #[test]
    fn parse_address_rejects_invalid_addresses() {
        let err = parse_address("not-an-address").unwrap_err();

        assert_eq!(err.exit_code(), 13);
        assert!(err.to_string().contains("Invalid address"));
    }

    #[test]
    fn parse_address_accepts_zero_prefixed_ethereum_addresses() {
        let address = parse_address("0x0000000000000000000000000000000000000001").unwrap();

        assert_eq!(
            address.to_string(),
            "0x0000000000000000000000000000000000000001"
        );
    }

    #[test]
    fn null_sequence_decode_is_treated_as_empty_subaccount_response() {
        let err = anyhow::anyhow!(
            "decode failed: invalid type: null, expected a sequence at line 1 column 4; body=null"
        );

        assert!(is_null_sequence_decode(&err));
    }
}
