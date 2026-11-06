//! Authenticated transfer commands.

use alloy::sol;
use clap::Args;
use hypersdk::hypercore::types::{Action, AssetTarget, SendAsset, SendToken, SpotSend, UsdSend};
use hypersdk::hypercore::{Chain, HttpClient, SpotToken};

use crate::signing::SelectedSigner;
use hypersdk::{Address, Decimal};
use serde::Serialize;

use crate::commands::actions;
use crate::commands::spot_balances::user_spot_balances_raw;
use crate::dry_run::ActionReversibility;
use crate::errors::CliError;
use crate::errors::from_sdk;
use crate::http_api::ApiClient;
use crate::invocation::{Invocation, Remote};
use crate::mode::Mode;
use crate::output::TableData;

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
    inv: &Invocation,
    signer: &SelectedSigner,
    balance_user: Address,
    args: &ClassTransferArgs,
) -> Result<(), anyhow::Error> {
    class_transfer(inv, signer, balance_user, args.amount, true, args.yes).await
}

/// Transfer USDC from perpetual balance to spot balance.
pub async fn perp_to_spot(
    inv: &Invocation,
    signer: &SelectedSigner,
    balance_user: Address,
    args: &ClassTransferArgs,
) -> Result<(), anyhow::Error> {
    class_transfer(inv, signer, balance_user, args.amount, false, args.yes).await
}

pub fn validate_class_transfer_args(args: &ClassTransferArgs) -> Result<(), CliError> {
    validate_positive_amount(args.amount)
}

pub fn validate_send_args(args: &SendArgs) -> Result<(), CliError> {
    validate_positive_amount(args.amount)?;
    parse_address(&args.to, "--to")?;
    Ok(())
}

pub fn validate_spot_send_args(args: &SpotSendArgs) -> Result<(), CliError> {
    validate_positive_amount(args.amount)?;
    parse_address(&args.to, "--to")?;
    validate_token_symbol(&args.token)?;
    Ok(())
}

pub fn validate_send_asset_args(args: &SendAssetArgs) -> Result<(), CliError> {
    validate_positive_amount(args.amount)?;
    parse_address(&args.to, "--to")?;
    validate_token_symbol(&args.token)?;
    parse_asset_target(&args.source, "--source")?;
    parse_asset_target(&args.dest, "--dest")?;
    if let Some(from_subaccount) = args.from_subaccount.as_deref() {
        parse_address(from_subaccount, "--from-subaccount")?;
    }
    Ok(())
}

pub fn validate_withdraw_args(args: &WithdrawArgs) -> Result<(), CliError> {
    validate_positive_amount(args.amount)?;
    parse_address(&args.to, "--to")?;
    Ok(())
}

/// Build the dry-run argument object for `transfer spot-send`.
pub async fn spot_send_dry_run_args(
    client: &HttpClient,
    args: &SpotSendArgs,
) -> Result<serde_json::Value, CliError> {
    let token = resolve_spot_token(client, &args.token).await?;
    Ok(serde_json::json!({
        "to": parse_address(&args.to, "--to")?.to_string(),
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
    let source = parse_asset_target(&args.source, "--source")?;
    let dest = parse_asset_target(&args.dest, "--dest")?;
    Ok(serde_json::json!({
        "to": parse_address(&args.to, "--to")?.to_string(),
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
    inv: &Invocation,
    signer: &SelectedSigner,
    balance_user: Address,
    args: &SendArgs,
) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let (api, chain, client) = (remote.api(), remote.chain(), remote.api().sdk());
    validate_positive_amount(args.amount)?;
    let destination = parse_address(&args.to, "--to")?;
    ensure_perp_balance(client, balance_user, args.amount).await?;
    let prompt = external_transfer_prompt(
        &format!("Send {} USDC to {}?", args.amount, args.to),
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
    confirm_transfer(inv.mode(), &prompt, "send transfer aborted", args.yes)?;

    let nonce = actions::nonce_now();
    let action = Action::UsdSend(
        UsdSend {
            destination,
            amount: args.amount,
            time: nonce,
        }
        .into_action(chain),
    );
    actions::send_l1_action(api, signer, action, nonce).await?;

    print_transfer(
        inv,
        "send",
        args.amount,
        "USDC".to_string(),
        Some(args.to.clone()),
    );
    Ok(())
}

/// Send a spot token to another Hyperliquid address.
pub async fn spot_send(
    inv: &Invocation,
    signer: &SelectedSigner,
    args: &SpotSendArgs,
) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let (api, chain, client) = (remote.api(), remote.chain(), remote.api().sdk());
    validate_spot_send_args(args)?;
    let destination = parse_address(&args.to, "--to")?;
    let token = resolve_spot_token(client, &args.token).await?;
    let prompt = external_transfer_prompt(
        &format!(
            "Send {} {} spot token to {}?",
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
    confirm_transfer(inv.mode(), &prompt, "spot send transfer aborted", args.yes)?;

    let nonce = actions::nonce_now();
    let spot_send = SpotSend {
        destination,
        token: SendToken(token.clone()),
        amount: args.amount,
        time: nonce,
    }
    .into_action(chain);
    let action = Action::SpotSend(spot_send);
    actions::send_l1_action(api, signer, action, nonce).await?;

    print_transfer(
        inv,
        "spot-send",
        args.amount,
        token.name,
        Some(args.to.clone()),
    );
    Ok(())
}

/// Send an asset between accounts, spot, perp, or DEX contexts.
pub async fn send_asset(
    inv: &Invocation,
    signer: &SelectedSigner,
    args: &SendAssetArgs,
) -> Result<(), anyhow::Error> {
    let token_name = execute_send_asset(inv.mode(), inv.remote()?, signer, args).await?;
    print_transfer(
        inv,
        "send-asset",
        args.amount,
        token_name,
        Some(args.to.clone()),
    );
    Ok(())
}

/// Sign and submit a `sendAsset` action without printing a receipt.
pub async fn execute_send_asset(
    mode: &Mode,
    remote: &Remote,
    signer: &SelectedSigner,
    args: &SendAssetArgs,
) -> Result<String, anyhow::Error> {
    let (api, chain, client) = (remote.api(), remote.chain(), remote.api().sdk());
    validate_send_asset_args(args)?;
    let destination = parse_address(&args.to, "--to")?;
    let token = resolve_spot_token(client, &args.token).await?;
    let source = parse_asset_target(&args.source, "--source")?;
    let dest = parse_asset_target(&args.dest, "--dest")?;
    let prompt = external_transfer_prompt(
        &format!(
            "Send {} {} from {} to {} for {}?",
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
    confirm_transfer(mode, &prompt, "send-asset transfer aborted", args.yes)?;

    let nonce = actions::nonce_now();
    let send_asset = SendAsset {
        destination,
        source_dex: source.target,
        destination_dex: dest.target,
        token: SendToken(token.clone()),
        amount: args.amount,
        from_sub_account: args.from_subaccount.clone().unwrap_or_default(),
        nonce,
    }
    .into_action(chain);
    let action = Action::SendAsset(send_asset);
    actions::send_l1_action(api, signer, action, nonce).await?;
    Ok(token.name)
}

/// Withdraw USDC to an Arbitrum address.
pub async fn withdraw(
    inv: &Invocation,
    signer: &SelectedSigner,
    balance_user: Address,
    args: &WithdrawArgs,
) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let (api, chain, client) = (remote.api(), remote.chain(), remote.api().sdk());
    validate_positive_amount(args.amount)?;
    let destination = parse_address(&args.to, "--to")?;
    ensure_perp_balance(client, balance_user, args.amount).await?;
    let prompt = external_transfer_prompt(
        &format!(
            "Withdraw {} USDC to Arbitrum address {}?",
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
    confirm_transfer(inv.mode(), &prompt, "withdraw transfer aborted", args.yes)?;

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
        api,
        serde_json::to_value(action).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?,
        nonce,
        signature,
    )
    .await?;

    print_transfer(
        inv,
        "withdraw",
        args.amount,
        "USDC".to_string(),
        Some(args.to.clone()),
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

fn parse_asset_target(input: &str, flag: &str) -> Result<ParsedAssetTarget, CliError> {
    let trimmed = input.trim();
    let (target, display, wire) = match trimmed {
        "" => {
            return Err(CliError::Configuration(format!(
                "{flag} must be perp, spot, or dex:<NAME>"
            )));
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
            return Err(CliError::Configuration(format!(
                "{flag} must be perp, spot, or dex:<NAME>"
            )));
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
    let tokens = client.spot_tokens().await.map_err(from_sdk)?;
    tokens
        .into_iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(token))
        .ok_or_else(|| CliError::Unsupported(format!("spot token \"{token}\" not found")))
}

fn parse_address(input: &str, flag: &str) -> Result<Address, CliError> {
    let stripped = input
        .strip_prefix("0x")
        .ok_or_else(|| invalid_address_error(flag))?;
    if stripped.len() != 40 || !stripped.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(invalid_address_error(flag));
    }
    if stripped.chars().all(|ch| ch == '0') {
        return Err(CliError::Configuration(format!(
            "{flag}: address must not be the zero address"
        )));
    }
    input
        .parse::<Address>()
        .map_err(|_| invalid_address_error(flag))
}

fn invalid_address_error(flag: &str) -> CliError {
    CliError::Configuration(format!(
        "{flag}: address must be a 0x-prefixed 40-byte hex string"
    ))
}

async fn class_transfer(
    inv: &Invocation,
    signer: &SelectedSigner,
    balance_user: Address,
    amount: Decimal,
    to_perp: bool,
    yes: bool,
) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let (api, chain, client) = (remote.api(), remote.chain(), remote.api().sdk());
    validate_positive_amount(amount)?;
    if to_perp {
        ensure_spot_usdc_balance(api, balance_user, amount).await?;
    } else {
        ensure_perp_balance(client, balance_user, amount).await?;
    }

    let prompt = if to_perp {
        format!("Transfer {amount} USDC from spot to perp?")
    } else {
        format!("Transfer {amount} USDC from perp to spot?")
    };
    let label = if to_perp {
        "spot-to-perp"
    } else {
        "perp-to-spot"
    };
    confirm_transfer(inv.mode(), &prompt, "transfer aborted", yes)?;

    let nonce = actions::nonce_now();
    let amount = amount.normalize().to_string();
    let message = UsdClassTransferMessage {
        signature_chain_id: chain.arbitrum_id().to_string(),
        hyperliquid_chain: chain,
        amount: amount.clone(),
        to_perp,
        nonce,
    };
    let signature = actions::sign_user_action::<UsdClassTransfer>(signer, chain, &message)?;
    let action = UsdClassTransferAction {
        action_type: "usdClassTransfer",
        signature_chain_id: chain.arbitrum_id().to_string(),
        hyperliquid_chain: chain,
        amount: amount.clone(),
        to_perp,
        nonce,
    };
    actions::send_user_signed_json_action(
        api,
        serde_json::to_value(action).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?,
        nonce,
        signature,
    )
    .await?;

    let printed_amount = amount.parse::<Decimal>().unwrap_or_default();
    print_transfer(inv, label, printed_amount, "USDC".to_string(), None);
    Ok(())
}

async fn ensure_spot_usdc_balance(
    api: &ApiClient,
    user: Address,
    amount: Decimal,
) -> Result<(), CliError> {
    let available = user_spot_balances_raw(api, user)
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
        .map_err(from_sdk)?
        .withdrawable;
    if available < amount {
        return Err(CliError::Unsupported(format!(
            "insufficient balance: requested {amount} USDC, available perp withdrawable {available}"
        )));
    }
    Ok(())
}

/// Ask for consent. `prompt` is an optional preview block followed by the
/// question on its last line. The block goes to stderr before the question.
fn confirm_transfer(
    mode: &Mode,
    prompt: &str,
    abort_message: &'static str,
    yes: bool,
) -> Result<(), CliError> {
    if yes {
        return Ok(());
    }
    crate::prompt::ensure_can_prompt(mode, "--yes")?;
    let (preview, question) = prompt.rsplit_once('\n').unwrap_or(("", prompt));
    if !preview.is_empty() {
        eprintln!("{preview}");
    }
    if crate::prompt::confirm_warning(mode, question, "--yes")? {
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
    inv: &Invocation,
    action: &str,
    amount: Decimal,
    asset: String,
    destination: Option<String>,
) {
    inv.print(&TransferOutput {
        rows: vec![TransferConfirmation {
            action: action.to_string(),
            status: "confirmed".to_string(),
            amount,
            asset,
            destination,
        }],
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_address_accepts_0x_40_byte_hex() {
        assert!(parse_address("0x0000000000000000000000000000000000000001", "--to").is_ok());
    }

    #[test]
    fn parse_address_rejects_zero_address_as_usage_error() {
        let err = parse_address("0x0000000000000000000000000000000000000000", "--to").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("zero address"));
    }

    #[test]
    fn parse_address_rejects_bad_format_as_usage_error() {
        let err = parse_address("INVALID", "--to").unwrap_err();
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
