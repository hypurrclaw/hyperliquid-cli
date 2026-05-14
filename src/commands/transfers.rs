//! Authenticated transfer commands.

use std::io::{self, Write};
use std::time::Instant;

use alloy::sol;
use clap::Args;
use hypersdk::hypercore::types::{Action, AssetTarget, SendAsset, SendToken, SpotSend, UsdSend};
use hypersdk::hypercore::{Chain, HttpClient, SpotToken};

use crate::signing::SelectedSigner;
use hypersdk::{Address, Decimal};
use serde::Serialize;

use crate::commands::actions;
use crate::commands::map_api_error;
use crate::commands::spot_balances::user_spot_balances_raw;
use crate::dry_run::ActionReversibility;
use crate::errors::CliError;
use crate::output::{self, OutputFormat, TableData};

sol! {
    struct UsdClassTransfer {
        string hyperliquidChain;
        string amount;
        bool toPerp;
        uint64 nonce;
    }

    struct Withdraw {
        string hyperliquidChain;
        string destination;
        string amount;
        uint64 time;
    }
}

/// Arguments for `transfer spot-to-perp`.
#[derive(Args, Debug, Clone)]
pub struct ClassTransferArgs {
    /// Amount of USDC to transfer
    #[arg(long, allow_hyphen_values = true)]
    pub amount: Decimal,

    /// Confirm live execution without prompting
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Arguments for `transfer send`.
#[derive(Args, Debug, Clone)]
pub struct SendArgs {
    /// Destination Hyperliquid address
    #[arg(long)]
    pub to: String,

    /// Amount of USDC to send
    #[arg(long, allow_hyphen_values = true)]
    pub amount: Decimal,

    /// Confirm live execution without prompting
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Arguments for `transfer spot-send`.
#[derive(Args, Debug, Clone)]
pub struct SpotSendArgs {
    /// Destination Hyperliquid address
    #[arg(long)]
    pub to: String,

    /// Spot token symbol, for example HYPE or PURR
    #[arg(long)]
    pub token: String,

    /// Token amount to send
    #[arg(long, allow_hyphen_values = true)]
    pub amount: Decimal,

    /// Confirm live execution without prompting
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Arguments for `transfer send-asset`.
#[derive(Args, Debug, Clone)]
pub struct SendAssetArgs {
    /// Destination Hyperliquid address
    #[arg(long)]
    pub to: String,

    /// Source venue: perp, spot, or dex:<NAME>
    #[arg(long)]
    pub source: String,

    /// Destination venue: perp, spot, or dex:<NAME>
    #[arg(long)]
    pub dest: String,

    /// Spot token symbol, for example USDC or HYPE
    #[arg(long)]
    pub token: String,

    /// Token amount to send
    #[arg(long, allow_hyphen_values = true)]
    pub amount: Decimal,

    /// Optional source subaccount address
    #[arg(long)]
    pub from_subaccount: Option<String>,

    /// Confirm live execution without prompting
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Arguments for `transfer withdraw`.
#[derive(Args, Debug, Clone)]
pub struct WithdrawArgs {
    /// Destination Arbitrum address
    #[arg(long)]
    pub to: String,

    /// Amount of USDC to withdraw
    #[arg(long, allow_hyphen_values = true)]
    pub amount: Decimal,

    /// Confirm live execution without prompting
    #[arg(short = 'y', long)]
    pub yes: bool,
}

#[derive(Debug, Clone, Serialize)]
struct TransferConfirmation {
    action: String,
    status: String,
    #[serde(with = "rust_decimal::serde::str")]
    amount: Decimal,
    asset: String,
    destination: Option<String>,
}

struct TransferOutput {
    rows: Vec<TransferConfirmation>,
}

impl TableData for TransferOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Action", "Status", "Amount", "Asset", "Destination"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.action.clone(),
                    row.status.clone(),
                    row.amount.to_string(),
                    row.asset.clone(),
                    row.destination.clone().unwrap_or_default(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsdClassTransferMessage {
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    amount: String,
    to_perp: bool,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsdClassTransferAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    amount: String,
    to_perp: bool,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WithdrawMessage {
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    destination: Address,
    amount: String,
    time: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WithdrawAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    destination: Address,
    amount: String,
    time: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferActionKind {
    SpotToPerp,
    PerpToSpot,
    Send,
    SpotSend,
    SendAsset,
    Withdraw,
}

impl TransferActionKind {
    #[must_use]
    pub fn reversibility(self) -> ActionReversibility {
        match self {
            Self::SpotToPerp | Self::PerpToSpot => ActionReversibility::Reversible,
            Self::Send | Self::SpotSend | Self::SendAsset | Self::Withdraw => {
                ActionReversibility::Irreversible
            }
        }
    }
}

struct TransferConfirmationContext {
    network: Chain,
    signer: Address,
    acting_context: Address,
    recipient: Option<Address>,
    destination_class: &'static str,
    asset: String,
    amount: Decimal,
    fee_or_cap: &'static str,
    reversibility: ActionReversibility,
}

/// Transfer USDC from spot balance to perpetual balance.
pub async fn spot_to_perp(
    api_base_url: &str,
    chain: Chain,
    client: &HttpClient,
    signer: &SelectedSigner,
    balance_user: Address,
    args: &ClassTransferArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    class_transfer(
        ClassTransferRuntime {
            api_base_url,
            chain,
            client,
            signer,
            balance_user,
        },
        args.amount,
        true,
        args.yes,
        format,
    )
    .await
}

/// Transfer USDC from perpetual balance to spot balance.
pub async fn perp_to_spot(
    api_base_url: &str,
    chain: Chain,
    client: &HttpClient,
    signer: &SelectedSigner,
    balance_user: Address,
    args: &ClassTransferArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    class_transfer(
        ClassTransferRuntime {
            api_base_url,
            chain,
            client,
            signer,
            balance_user,
        },
        args.amount,
        false,
        args.yes,
        format,
    )
    .await
}

pub fn validate_class_transfer_args(args: &ClassTransferArgs) -> Result<(), CliError> {
    validate_positive_amount(args.amount)
}

pub fn validate_send_args(args: &SendArgs) -> Result<(), CliError> {
    validate_positive_amount(args.amount)?;
    parse_address(&args.to)?;
    Ok(())
}

pub fn validate_spot_send_args(args: &SpotSendArgs) -> Result<(), CliError> {
    validate_positive_amount(args.amount)?;
    parse_address(&args.to)?;
    validate_token_symbol(&args.token)?;
    Ok(())
}

pub fn validate_send_asset_args(args: &SendAssetArgs) -> Result<(), CliError> {
    validate_positive_amount(args.amount)?;
    parse_address(&args.to)?;
    validate_token_symbol(&args.token)?;
    parse_asset_target(&args.source)?;
    parse_asset_target(&args.dest)?;
    if let Some(from_subaccount) = args.from_subaccount.as_deref() {
        parse_address(from_subaccount)?;
    }
    Ok(())
}

pub fn validate_withdraw_args(args: &WithdrawArgs) -> Result<(), CliError> {
    validate_positive_amount(args.amount)?;
    parse_address(&args.to)?;
    Ok(())
}

/// Build the dry-run argument object for `transfer spot-send`.
pub async fn spot_send_dry_run_args(
    client: &HttpClient,
    args: &SpotSendArgs,
) -> Result<serde_json::Value, CliError> {
    let token = resolve_spot_token(client, &args.token).await?;
    Ok(serde_json::json!({
        "to": parse_address(&args.to)?.to_string(),
        "token": token.name,
        "token_index": token.index,
        "amount": args.amount.normalize().to_string(),
    }))
}

/// Build the dry-run argument object for `transfer send-asset`.
pub async fn send_asset_dry_run_args(
    client: &HttpClient,
    args: &SendAssetArgs,
) -> Result<serde_json::Value, CliError> {
    let token = resolve_spot_token(client, &args.token).await?;
    let source = parse_asset_target(&args.source)?;
    let dest = parse_asset_target(&args.dest)?;
    Ok(serde_json::json!({
        "to": parse_address(&args.to)?.to_string(),
        "source": source.display,
        "source_wire": source.wire,
        "dest": dest.display,
        "dest_wire": dest.wire,
        "token": token.name,
        "token_index": token.index,
        "amount": args.amount.normalize().to_string(),
        "from_subaccount": args.from_subaccount.clone(),
    }))
}

/// Send USDC from perp balance to another Hyperliquid address.
pub async fn send(
    api_base_url: &str,
    chain: Chain,
    client: &HttpClient,
    signer: &SelectedSigner,
    balance_user: Address,
    args: &SendArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    validate_positive_amount(args.amount)?;
    let destination = parse_address(&args.to)?;
    let start = Instant::now();
    ensure_perp_balance(client, balance_user, args.amount).await?;
    let prompt = external_transfer_prompt(
        &format!("Send {} USDC to {}? [y/N] ", args.amount, args.to),
        TransferConfirmationContext {
            network: chain,
            signer: signer.address(),
            acting_context: balance_user,
            recipient: Some(destination),
            destination_class: "hyperliquid_user_address",
            asset: "USDC".to_string(),
            amount: args.amount,
            fee_or_cap: "not_estimated_exchange_default",
            reversibility: TransferActionKind::Send.reversibility(),
        },
    );
    confirm_transfer(&prompt, "send transfer aborted", format, args.yes)?;

    let nonce = actions::nonce_now();
    let action = Action::UsdSend(
        UsdSend {
            destination,
            amount: args.amount,
            time: nonce,
        }
        .into_action(chain),
    );
    actions::send_l1_action(api_base_url, chain, signer, action, nonce).await?;

    print_transfer(
        "send",
        args.amount,
        "USDC".to_string(),
        Some(args.to.clone()),
        format,
        start,
    );
    Ok(())
}

/// Send a spot token to another Hyperliquid address.
pub async fn spot_send(
    api_base_url: &str,
    chain: Chain,
    client: &HttpClient,
    signer: &SelectedSigner,
    args: &SpotSendArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    validate_spot_send_args(args)?;
    let destination = parse_address(&args.to)?;
    let token = resolve_spot_token(client, &args.token).await?;
    let start = Instant::now();
    let prompt = external_transfer_prompt(
        &format!(
            "Send {} {} spot token to {}? [y/N] ",
            args.amount, token.name, args.to
        ),
        TransferConfirmationContext {
            network: chain,
            signer: signer.address(),
            acting_context: signer.query_address(),
            recipient: Some(destination),
            destination_class: "hyperliquid_spot_user_address",
            asset: token.name.clone(),
            amount: args.amount,
            fee_or_cap: "not_estimated_exchange_default",
            reversibility: TransferActionKind::SpotSend.reversibility(),
        },
    );
    confirm_transfer(&prompt, "spot send transfer aborted", format, args.yes)?;

    let nonce = actions::nonce_now();
    let action = Action::SpotSend(
        SpotSend {
            destination,
            token: SendToken(token.clone()),
            amount: args.amount,
            time: nonce,
        }
        .into_action(chain),
    );
    actions::send_l1_action(api_base_url, chain, signer, action, nonce).await?;

    print_transfer(
        "spot-send",
        args.amount,
        token.name,
        Some(args.to.clone()),
        format,
        start,
    );
    Ok(())
}

/// Send an asset between accounts, spot, perp, or DEX contexts.
pub async fn send_asset(
    api_base_url: &str,
    chain: Chain,
    client: &HttpClient,
    signer: &SelectedSigner,
    args: &SendAssetArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    validate_send_asset_args(args)?;
    let destination = parse_address(&args.to)?;
    let token = resolve_spot_token(client, &args.token).await?;
    let source = parse_asset_target(&args.source)?;
    let dest = parse_asset_target(&args.dest)?;
    let start = Instant::now();
    let prompt = external_transfer_prompt(
        &format!(
            "Send {} {} from {} to {} for {}? [y/N] ",
            args.amount, token.name, source.display, dest.display, args.to
        ),
        TransferConfirmationContext {
            network: chain,
            signer: signer.address(),
            acting_context: signer.query_address(),
            recipient: Some(destination),
            destination_class: "hyperliquid_asset_context",
            asset: format!("{} {}->{}", token.name, source.display, dest.display),
            amount: args.amount,
            fee_or_cap: "not_estimated_exchange_default",
            reversibility: TransferActionKind::SendAsset.reversibility(),
        },
    );
    confirm_transfer(&prompt, "send-asset transfer aborted", format, args.yes)?;

    let nonce = actions::nonce_now();
    let action = Action::SendAsset(
        SendAsset {
            destination,
            source_dex: source.target,
            destination_dex: dest.target,
            token: SendToken(token.clone()),
            amount: args.amount,
            from_sub_account: args.from_subaccount.clone().unwrap_or_default(),
            nonce,
        }
        .into_action(chain),
    );
    actions::send_l1_action(api_base_url, chain, signer, action, nonce).await?;

    print_transfer(
        "send-asset",
        args.amount,
        token.name,
        Some(args.to.clone()),
        format,
        start,
    );
    Ok(())
}

/// Withdraw USDC to an Arbitrum address.
pub async fn withdraw(
    api_base_url: &str,
    chain: Chain,
    client: &HttpClient,
    signer: &SelectedSigner,
    balance_user: Address,
    args: &WithdrawArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    validate_positive_amount(args.amount)?;
    let destination = parse_address(&args.to)?;
    let start = Instant::now();
    ensure_perp_balance(client, balance_user, args.amount).await?;
    let prompt = external_transfer_prompt(
        &format!(
            "Withdraw {} USDC to Arbitrum address {}? [y/N] ",
            args.amount, args.to
        ),
        TransferConfirmationContext {
            network: chain,
            signer: signer.address(),
            acting_context: balance_user,
            recipient: Some(destination),
            destination_class: "arbitrum_address",
            asset: "USDC".to_string(),
            amount: args.amount,
            fee_or_cap: "not_estimated_bridge_or_exchange_fee",
            reversibility: TransferActionKind::Withdraw.reversibility(),
        },
    );
    confirm_transfer(&prompt, "withdraw transfer aborted", format, args.yes)?;

    let nonce = actions::nonce_now();
    let message = WithdrawMessage {
        signature_chain_id: chain.arbitrum_id().to_string(),
        hyperliquid_chain: chain,
        destination,
        amount: args.amount.normalize().to_string(),
        time: nonce,
    };
    let signature = actions::sign_user_action::<Withdraw>(signer, chain, &message)?;
    let action = WithdrawAction {
        action_type: "withdraw3",
        signature_chain_id: chain.arbitrum_id().to_string(),
        hyperliquid_chain: chain,
        destination,
        amount: args.amount.normalize().to_string(),
        time: nonce,
    };
    actions::send_user_signed_json_action(
        api_base_url,
        serde_json::to_value(action).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?,
        nonce,
        signature,
    )
    .await?;

    print_transfer(
        "withdraw",
        args.amount,
        "USDC".to_string(),
        Some(args.to.clone()),
        format,
        start,
    );
    Ok(())
}

fn validate_positive_amount(amount: Decimal) -> Result<(), CliError> {
    if amount <= Decimal::ZERO {
        return Err(CliError::Configuration(
            "amount must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

fn validate_token_symbol(token: &str) -> Result<(), CliError> {
    if token.trim().is_empty() {
        return Err(CliError::Configuration("token cannot be empty".to_string()));
    }
    Ok(())
}

#[derive(Debug)]
struct ParsedAssetTarget {
    target: AssetTarget,
    display: String,
    wire: String,
}

fn parse_asset_target(input: &str) -> Result<ParsedAssetTarget, CliError> {
    let trimmed = input.trim();
    let (target, display, wire) = match trimmed {
        "" => {
            return Err(CliError::Configuration(
                "asset target must be perp, spot, or dex:<NAME>".to_string(),
            ));
        }
        // Hyperliquid encodes the default perp venue as an empty string in sendAsset.
        "perp" => (AssetTarget::Perp, "perp".to_string(), String::new()),
        "spot" => (AssetTarget::Spot, "spot".to_string(), "spot".to_string()),
        value if value.starts_with("dex:") => {
            let dex = value.trim_start_matches("dex:");
            if dex.is_empty() {
                return Err(CliError::Configuration(
                    "asset target dex:<NAME> requires a DEX name".to_string(),
                ));
            }
            (
                AssetTarget::Dex(dex.to_string()),
                value.to_string(),
                dex.to_string(),
            )
        }
        _ => {
            return Err(CliError::Configuration(
                "asset target must be perp, spot, or dex:<NAME>".to_string(),
            ));
        }
    };
    Ok(ParsedAssetTarget {
        target,
        display,
        wire,
    })
}

async fn resolve_spot_token(client: &HttpClient, token: &str) -> Result<SpotToken, CliError> {
    let token = token.trim();
    validate_token_symbol(token)?;
    let tokens = client.spot_tokens().await.map_err(map_api_error)?;
    tokens
        .into_iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(token))
        .ok_or_else(|| CliError::Unsupported(format!("spot token \"{token}\" not found")))
}

fn parse_address(input: &str) -> Result<Address, CliError> {
    let stripped = input.strip_prefix("0x").ok_or_else(invalid_address_error)?;
    if stripped.len() != 40 || !stripped.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(invalid_address_error());
    }
    if stripped.chars().all(|ch| ch == '0') {
        return Err(CliError::Configuration(
            "address must not be the zero address".to_string(),
        ));
    }
    input
        .parse::<Address>()
        .map_err(|_| invalid_address_error())
}

fn invalid_address_error() -> CliError {
    CliError::Configuration("address must be a 0x-prefixed 40-byte hex string".to_string())
}

struct ClassTransferRuntime<'a> {
    api_base_url: &'a str,
    chain: Chain,
    client: &'a HttpClient,
    signer: &'a SelectedSigner,
    balance_user: Address,
}

async fn class_transfer(
    runtime: ClassTransferRuntime<'_>,
    amount: Decimal,
    to_perp: bool,
    yes: bool,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    validate_positive_amount(amount)?;
    let start = Instant::now();
    if to_perp {
        ensure_spot_usdc_balance(runtime.client, runtime.balance_user, amount).await?;
    } else {
        ensure_perp_balance(runtime.client, runtime.balance_user, amount).await?;
    }

    let prompt = if to_perp {
        format!("Transfer {amount} USDC from spot to perp? [y/N] ")
    } else {
        format!("Transfer {amount} USDC from perp to spot? [y/N] ")
    };
    let label = if to_perp {
        "spot-to-perp"
    } else {
        "perp-to-spot"
    };
    confirm_transfer(&prompt, "transfer aborted", format, yes)?;

    let nonce = actions::nonce_now();
    let amount = amount.normalize().to_string();
    let message = UsdClassTransferMessage {
        signature_chain_id: runtime.chain.arbitrum_id().to_string(),
        hyperliquid_chain: runtime.chain,
        amount: amount.clone(),
        to_perp,
        nonce,
    };
    let signature =
        actions::sign_user_action::<UsdClassTransfer>(runtime.signer, runtime.chain, &message)?;
    let action = UsdClassTransferAction {
        action_type: "usdClassTransfer",
        signature_chain_id: runtime.chain.arbitrum_id().to_string(),
        hyperliquid_chain: runtime.chain,
        amount: amount.clone(),
        to_perp,
        nonce,
    };
    actions::send_user_signed_json_action(
        runtime.api_base_url,
        serde_json::to_value(action).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?,
        nonce,
        signature,
    )
    .await?;

    let printed_amount = amount.parse::<Decimal>().unwrap_or_default();
    print_transfer(
        label,
        printed_amount,
        "USDC".to_string(),
        None,
        format,
        start,
    );
    Ok(())
}

async fn ensure_spot_usdc_balance(
    client: &HttpClient,
    user: Address,
    amount: Decimal,
) -> Result<(), CliError> {
    let available = user_spot_balances_raw(client, user)
        .await?
        .into_iter()
        .find(|balance| balance.coin.eq_ignore_ascii_case("USDC"))
        .map(|balance| balance.total - balance.hold)
        .unwrap_or_default();
    if available < amount {
        return Err(CliError::Unsupported(format!(
            "insufficient balance: requested {amount} USDC, available spot USDC {available}"
        )));
    }
    Ok(())
}

async fn ensure_perp_balance(
    client: &HttpClient,
    user: Address,
    amount: Decimal,
) -> Result<(), CliError> {
    let available = client
        .clearinghouse_state(user, None)
        .await
        .map_err(map_api_error)?
        .withdrawable;
    if available < amount {
        return Err(CliError::Unsupported(format!(
            "insufficient balance: requested {amount} USDC, available perp withdrawable {available}"
        )));
    }
    Ok(())
}

fn confirm_transfer(
    prompt: &str,
    abort_message: &'static str,
    format: OutputFormat,
    yes: bool,
) -> Result<(), CliError> {
    if yes {
        return Ok(());
    }
    let mut stderr = io::stderr();
    let prompt = if format == OutputFormat::Pretty {
        output::colors::yellow(prompt)
    } else {
        prompt.to_string()
    };
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
        Err(CliError::Configuration(abort_message.to_string()))
    }
}

fn external_transfer_prompt(summary: &str, context: TransferConfirmationContext) -> String {
    format!(
        "\
Network: {network}
Signer: {signer}
Acting context: {acting_context}
Recipient: {recipient}
Destination class: {destination_class}
Asset: {asset}
Amount: {amount}
Fee/cap: {fee_or_cap}
Reversibility: {reversibility}
{summary}",
        network = context.network,
        signer = context.signer,
        acting_context = context.acting_context,
        recipient = context
            .recipient
            .map(|recipient| recipient.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        destination_class = context.destination_class,
        asset = context.asset,
        amount = context.amount,
        fee_or_cap = context.fee_or_cap,
        reversibility = format_reversibility(context.reversibility),
        summary = summary,
    )
}

fn format_reversibility(reversibility: ActionReversibility) -> &'static str {
    match reversibility {
        ActionReversibility::Reversible => "reversible",
        ActionReversibility::PartiallyReversible => "partially_reversible",
        ActionReversibility::Irreversible => "irreversible",
    }
}

fn print_transfer(
    action: &str,
    amount: Decimal,
    asset: String,
    destination: Option<String>,
    format: OutputFormat,
    start: Instant,
) {
    output::print_data(
        &TransferOutput {
            rows: vec![TransferConfirmation {
                action: action.to_string(),
                status: "confirmed".to_string(),
                amount,
                asset,
                destination,
            }],
        },
        format,
        start.elapsed(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_address_accepts_0x_40_byte_hex() {
        assert!(parse_address("0x0000000000000000000000000000000000000001").is_ok());
    }

    #[test]
    fn parse_address_rejects_zero_address_as_usage_error() {
        let err = parse_address("0x0000000000000000000000000000000000000000").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("zero address"));
    }

    #[test]
    fn parse_address_rejects_bad_format_as_usage_error() {
        let err = parse_address("INVALID").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("0x-prefixed"));
    }

    #[test]
    fn transfer_actions_declare_reversibility() {
        assert_eq!(
            TransferActionKind::SpotToPerp.reversibility(),
            ActionReversibility::Reversible
        );
        assert_eq!(
            TransferActionKind::PerpToSpot.reversibility(),
            ActionReversibility::Reversible
        );
        assert_eq!(
            TransferActionKind::Send.reversibility(),
            ActionReversibility::Irreversible
        );
        assert_eq!(
            TransferActionKind::SpotSend.reversibility(),
            ActionReversibility::Irreversible
        );
        assert_eq!(
            TransferActionKind::SendAsset.reversibility(),
            ActionReversibility::Irreversible
        );
        assert_eq!(
            TransferActionKind::Withdraw.reversibility(),
            ActionReversibility::Irreversible
        );
    }
}
