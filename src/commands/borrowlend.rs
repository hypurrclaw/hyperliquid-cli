//! Borrow/lend public market data commands.

use clap::Args;
use hypersdk::hypercore::HttpClient;
use hypersdk::{Address, Decimal};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use crate::command_spec::{CliCommand, CommandSpec, GroupSpec, InputKind, NoArgs, arg};
use crate::commands::account::OptionalAddressArgs;
use crate::commands::actions;
use crate::commands::spot_balances::{RawSpotBalance, user_spot_balances_raw};
use crate::commands::support::{
    default_dry_run_signing_context, print_dry_run_envelope, signed_action_dry_run_envelope,
};
use crate::dry_run::ActionReversibility;
use crate::errors::CliError;
use crate::errors::from_sdk;
use crate::http_api::ApiClient;
use crate::invocation::Invocation;
use crate::output::{self, TableData};
use crate::signing::SelectedSigner;

const LIVE_RESERVE_NOTE: &str = "Live reserve state from Hyperliquid API";
const UNAVAILABLE_RESERVE_NOTE: &str =
    "Reserve state rates unavailable from public Hyperliquid API";
const CORE_WRITER_ACTION_ID: u32 = 15;

#[derive(Debug, Clone, PartialEq, Serialize)]
struct ReserveRow {
    token: String,
    token_index: u32,
    #[serde(with = "rust_decimal::serde::str")]
    supply_rate: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    borrow_rate: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    total_supply: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    total_borrow: Decimal,
    note: String,
}

#[derive(Args, Debug, Clone)]
pub struct ActionArgs {
    /// Borrow/lend reserve token, for example USDC
    pub token: String,

    /// Human-readable token amount. Omit with --max for maximum withdraw.
    #[arg(long, allow_hyphen_values = true, conflicts_with = "max")]
    pub amount: Option<Decimal>,

    /// Maximum amount. Encodes wei=0 in the verified CoreWriter action shape.
    #[arg(long, conflicts_with = "amount")]
    pub max: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BorrowLendOperation {
    Supply,
    Withdraw,
}

impl BorrowLendOperation {
    fn encoded(self) -> u8 {
        match self {
            Self::Supply => 0,
            Self::Withdraw => 1,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supply => "supply",
            Self::Withdraw => "withdraw",
        }
    }

    #[must_use]
    pub fn reversibility(self) -> ActionReversibility {
        ActionReversibility::PartiallyReversible
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawReserveState {
    #[serde(with = "rust_decimal::serde::str")]
    borrow_yearly_rate: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    supply_yearly_rate: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    total_supplied: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    total_borrowed: Decimal,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum BorrowLendInfoRequest {
    #[serde(rename = "allBorrowLendReserveStates")]
    AllReserveStates,
    #[serde(rename = "borrowLendReserveState")]
    ReserveState { token: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReserveOutput {
    rows: Vec<ReserveRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct UserBorrowLendPosition {
    token: String,
    token_index: usize,
    #[serde(with = "rust_decimal::serde::str")]
    supplied: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    borrowed: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    hold: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserBorrowLendOutput {
    user: String,
    positions: Vec<UserBorrowLendPosition>,
}

#[derive(Debug, Clone, Serialize)]
struct BorrowLendActionRow {
    status: String,
    action: String,
    operation: String,
    acting_as: String,
    token: String,
    token_index: u32,
    amount: Option<String>,
    max: bool,
    network: String,
    signer: String,
    reversibility: String,
}

struct BorrowLendActionOutput {
    row: BorrowLendActionRow,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BorrowLendExchangeAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    operation: &'static str,
    token: u32,
    amount: Option<String>,
}

impl TableData for ReserveOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Token",
            "Supply Rate",
            "Borrow Rate",
            "Total Supply",
            "Total Borrow",
            "Note",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.token.clone(),
                    row.supply_rate.to_string(),
                    row.borrow_rate.to_string(),
                    row.total_supply.to_string(),
                    row.total_borrow.to_string(),
                    row.note.clone(),
                ]
            })
            .collect()
    }

    fn pretty_rows(&self) -> Vec<Vec<String>> {
        self.rows()
            .into_iter()
            .map(|mut row| {
                row[5] = output::colors::gray(&row[5]);
                row
            })
            .collect()
    }

    fn empty_message(&self) -> Option<&str> {
        Some("No reserve states.")
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

impl TableData for UserBorrowLendOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["User", "Token", "Supplied", "Borrowed", "Hold"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.positions
            .iter()
            .map(|row| {
                vec![
                    self.user.clone(),
                    row.token.clone(),
                    row.supplied.to_string(),
                    row.borrowed.to_string(),
                    row.hold.to_string(),
                ]
            })
            .collect()
    }

    fn empty_message(&self) -> Option<&str> {
        Some("No borrow/lend positions.")
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "user": self.user,
            "positions": self.positions,
        })
    }
}

impl TableData for BorrowLendActionOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Status",
            "Action",
            "Operation",
            "Acting As",
            "Token",
            "Token Index",
            "Amount",
            "Max",
            "Network",
            "Signer",
            "Reversibility",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.row.status.clone(),
            self.row.action.clone(),
            self.row.operation.clone(),
            self.row.acting_as.clone(),
            self.row.token.clone(),
            self.row.token_index.to_string(),
            self.row.amount.clone().unwrap_or_default(),
            self.row.max.to_string(),
            self.row.network.clone(),
            self.row.signer.clone(),
            self.row.reversibility.clone(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.row).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// Show reserve states for all borrow/lend tokens.
pub async fn rates(inv: &Invocation) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let rows = reserve_rows(remote.api()).await?;
    inv.print(&ReserveOutput { rows });
    Ok(())
}

/// Show reserve state for a single token.
pub async fn get(inv: &Invocation, token: &str) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let token = resolve_spot_token(remote.api().sdk(), token).await?;
    let row = match live_reserve_row(remote.api(), token.index, token.name.clone()).await {
        Ok(row) => row,
        Err(BorrowLendInfoError::EndpointUnavailable) => unavailable_reserve_row(token),
        Err(BorrowLendInfoError::Cli(err)) => return Err(err.into()),
    };

    inv.print(&ReserveOutput { rows: vec![row] });
    Ok(())
}

/// Show public borrow/lend state for a user.
pub async fn user(inv: &Invocation, address: &str) -> Result<(), anyhow::Error> {
    let api = inv.remote()?.api();
    let user = parse_address(address)?;
    let balances = user_spot_balances_raw(api, user).await?;
    let positions = balances
        .into_iter()
        .filter(|balance| balance.token.is_some())
        .filter(|balance| !balance.total.is_zero() || !balance.hold.is_zero())
        .map(UserBorrowLendPosition::from)
        .collect::<Vec<_>>();

    inv.print(&UserBorrowLendOutput {
        user: address.to_string(),
        positions,
    });
    Ok(())
}

pub async fn action_preview(
    api: &ApiClient,
    operation: BorrowLendOperation,
    args: &ActionArgs,
) -> Result<serde_json::Value, CliError> {
    let chain = api.chain();
    validate_action_args(operation, args)?;
    let token = resolve_spot_token(api.sdk(), &args.token).await?;
    let wei = borrowlend_wei(args, token.wei_decimals)?;
    Ok(serde_json::json!({
        "operation": operation.as_str(),
        "encoded_operation": operation.encoded(),
        "token": token.name,
        "token_index": token.index,
        "amount": args.amount.map(|amount| amount.to_string()),
        "max": args.max,
        "network": chain.to_string(),
        "wei": wei,
        "reversibility": operation.reversibility(),
        "verified_shape": {
            "transport": "HyperEVM CoreWriter.sendRawAction",
            "action_id": CORE_WRITER_ACTION_ID,
            "encoding_version": 1,
            "solidity_types": ["uint8", "uint64", "uint64"],
            "fields": {
                "encodedOperation": operation.encoded(),
                "token": token.index,
                "wei": wei
            }
        },
        "exchange_action": {
            "type": "borrowLend",
            "operation": operation.as_str(),
            "token": token.index,
            "amount": borrowlend_exchange_amount(args)
        },
        "live_submission": "enabled: wallet-signed /exchange borrowLend action shape verified against @nktkas/hyperliquid v0.31.0"
    }))
}

pub async fn action(
    inv: &Invocation,
    signer: &SelectedSigner,
    operation: BorrowLendOperation,
    args: &ActionArgs,
) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let client = remote.api().sdk();
    let chain = remote.chain();
    validate_action_args(operation, args)?;
    let token = resolve_spot_token(client, &args.token).await?;
    let amount = borrowlend_exchange_amount(args);

    // Verified against @nktkas/hyperliquid v0.31.0 exchange `borrowLend` schema:
    // { type: "borrowLend", operation: "supply"|"withdraw", token, amount: null|string }.
    let action = BorrowLendExchangeAction {
        action_type: "borrowLend",
        operation: operation.as_str(),
        token: token.index,
        amount: amount.clone(),
    };
    actions::send_raw_l1_json_action(
        remote.api(),
        signer,
        &action,
        actions::RawL1ActionMetadata::new(actions::nonce_now()),
        "borrow/lend action failed",
    )
    .await?;

    inv.print(&BorrowLendActionOutput {
        row: BorrowLendActionRow {
            status: "submitted".to_string(),
            action: "borrowlend".to_string(),
            operation: operation.as_str().to_string(),
            acting_as: signer.query_address().to_string(),
            token: token.name,
            token_index: token.index,
            amount,
            max: args.max,
            network: chain.to_string(),
            signer: signer.address().to_string(),
            reversibility: format_reversibility(operation.reversibility()).to_string(),
        },
    });
    Ok(())
}

pub fn validate_action_args(
    operation: BorrowLendOperation,
    args: &ActionArgs,
) -> Result<(), CliError> {
    let has_amount = args.amount.is_some();
    if !has_amount && !args.max {
        return Err(CliError::Unsupported(
            "borrow/lend action requires --amount <AMOUNT> or --max".to_string(),
        ));
    }
    if operation == BorrowLendOperation::Supply && args.max {
        return Err(CliError::Unsupported(
            "borrow/lend supply requires --amount; --max is only supported for withdraw"
                .to_string(),
        ));
    }
    if let Some(amount) = args.amount
        && amount <= Decimal::ZERO
    {
        return Err(CliError::Unsupported(
            "borrow/lend amount must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

async fn reserve_rows(api: &ApiClient) -> Result<Vec<ReserveRow>, CliError> {
    let mut rows = match live_reserve_rows(api).await {
        Ok(rows) => rows,
        Err(BorrowLendInfoError::EndpointUnavailable) => {
            unavailable_reserve_rows(api.sdk()).await?
        }
        Err(BorrowLendInfoError::Cli(err)) => return Err(err),
    };

    rows.sort_by(|left, right| {
        (left.token != "USDC")
            .cmp(&(right.token != "USDC"))
            .then_with(|| left.token.cmp(&right.token))
    });

    Ok(rows)
}

async fn live_reserve_rows(api: &ApiClient) -> Result<Vec<ReserveRow>, BorrowLendInfoError> {
    let reserve_states =
        post_info::<Vec<(u32, RawReserveState)>>(api, &BorrowLendInfoRequest::AllReserveStates)
            .await?;
    let token_names = token_names_by_index(api.sdk()).await?;

    Ok(reserve_states
        .into_iter()
        .map(|(token_index, reserve)| {
            reserve.to_row(
                token_names
                    .get(&token_index)
                    .cloned()
                    .unwrap_or_else(|| format!("#{token_index}")),
                token_index,
            )
        })
        .collect())
}

async fn live_reserve_row(
    api: &ApiClient,
    token_index: u32,
    token_name: String,
) -> Result<ReserveRow, BorrowLendInfoError> {
    let reserve_state = post_info::<RawReserveState>(
        api,
        &BorrowLendInfoRequest::ReserveState { token: token_index },
    )
    .await?;
    Ok(reserve_state.to_row(token_name, token_index))
}

async fn unavailable_reserve_rows(client: &HttpClient) -> Result<Vec<ReserveRow>, CliError> {
    Ok(client
        .spot_tokens()
        .await
        .map_err(from_sdk)?
        .into_iter()
        .map(unavailable_reserve_row)
        .collect::<Vec<_>>())
}

fn unavailable_reserve_row(token: hypersdk::hypercore::SpotToken) -> ReserveRow {
    ReserveRow {
        token: token.name,
        token_index: token.index,
        supply_rate: Decimal::ZERO,
        borrow_rate: Decimal::ZERO,
        total_supply: Decimal::ZERO,
        total_borrow: Decimal::ZERO,
        note: UNAVAILABLE_RESERVE_NOTE.to_string(),
    }
}

impl RawReserveState {
    fn to_row(&self, token: String, token_index: u32) -> ReserveRow {
        ReserveRow {
            token,
            token_index,
            supply_rate: self.supply_yearly_rate,
            borrow_rate: self.borrow_yearly_rate,
            total_supply: self.total_supplied,
            total_borrow: self.total_borrowed,
            note: LIVE_RESERVE_NOTE.to_string(),
        }
    }
}

async fn token_names_by_index(
    client: &HttpClient,
) -> Result<std::collections::HashMap<u32, String>, BorrowLendInfoError> {
    Ok(client
        .spot_tokens()
        .await
        .map_err(from_sdk)
        .map_err(BorrowLendInfoError::Cli)?
        .into_iter()
        .map(|token| (token.index, token.name))
        .collect())
}

async fn resolve_spot_token(
    client: &HttpClient,
    token: &str,
) -> Result<hypersdk::hypercore::SpotToken, CliError> {
    client
        .spot_tokens()
        .await
        .map_err(from_sdk)?
        .into_iter()
        .find(|spot_token| spot_token.name.eq_ignore_ascii_case(token))
        .ok_or_else(|| CliError::Unsupported(format!("Borrow/lend token not found: {token}")))
}

fn borrowlend_wei(args: &ActionArgs, decimals: i64) -> Result<u64, CliError> {
    if args.max {
        return Ok(0);
    }
    let amount = args.amount.ok_or_else(|| {
        CliError::Configuration("amount is required when --max is not set".to_string())
    })?;
    decimal_to_wei(amount, decimals)
}

fn borrowlend_exchange_amount(args: &ActionArgs) -> Option<String> {
    if args.max {
        None
    } else {
        args.amount.map(|amount| amount.normalize().to_string())
    }
}

fn decimal_to_wei(amount: Decimal, decimals: i64) -> Result<u64, CliError> {
    if decimals < 0 {
        return Err(CliError::Unsupported(
            "token decimals cannot be negative".to_string(),
        ));
    }
    let decimals = u32::try_from(decimals).map_err(|_| {
        CliError::Unsupported("token decimals are outside supported bounds".to_string())
    })?;
    let multiplier = 10u64.checked_pow(decimals).ok_or_else(|| {
        CliError::Unsupported("token decimals are outside supported bounds".to_string())
    })?;
    let multiplier = Decimal::from(multiplier);
    let wei = amount * multiplier;
    if wei.fract() != Decimal::ZERO {
        return Err(CliError::Unsupported(format!(
            "amount {amount} has more precision than token supports ({decimals} decimals)"
        )));
    }
    wei.to_u64()
        .ok_or_else(|| CliError::Unsupported("borrow/lend amount is too large".to_string()))
}

#[derive(Debug)]
enum BorrowLendInfoError {
    Cli(CliError),
    EndpointUnavailable,
}

impl From<CliError> for BorrowLendInfoError {
    fn from(err: CliError) -> Self {
        Self::Cli(err)
    }
}

async fn post_info<T: for<'de> Deserialize<'de>>(
    api: &ApiClient,
    request: &BorrowLendInfoRequest,
) -> Result<T, BorrowLendInfoError> {
    let reply = api.info_raw(request).await?;
    if info_request_type_is_unavailable(reply.status().as_u16(), reply.body()) {
        return Err(BorrowLendInfoError::EndpointUnavailable);
    }
    reply.decode("borrow/lend").map_err(Into::into)
}

fn info_request_type_is_unavailable(status_code: u16, body: &str) -> bool {
    if (200..300).contains(&status_code) {
        return structured_unknown_request_error(body);
    }

    let lower = body.to_ascii_lowercase();
    (status_code == 400 || status_code == 404 || status_code == 422)
        && (lower.contains("unknown")
            || lower.contains("unsupported")
            || lower.contains("invalid request")
            || lower.contains("invalid type"))
        && (lower.contains("borrowlend")
            || lower.contains("borrow/lend")
            || lower.contains("request")
            || lower.contains("type"))
}

fn structured_unknown_request_error(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };

    let status_is_error = value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|status| {
            matches!(
                status.to_ascii_lowercase().as_str(),
                "err" | "error" | "failed" | "failure"
            )
        });

    status_is_error && info_request_type_is_unavailable(400, &value.to_string())
}

fn format_reversibility(reversibility: ActionReversibility) -> &'static str {
    match reversibility {
        ActionReversibility::Reversible => "reversible",
        ActionReversibility::PartiallyReversible => "partially_reversible",
        ActionReversibility::Irreversible => "irreversible",
    }
}

impl From<RawSpotBalance> for UserBorrowLendPosition {
    fn from(balance: RawSpotBalance) -> Self {
        let (supplied, borrowed) = if balance.total < Decimal::ZERO {
            (Decimal::ZERO, -balance.total)
        } else {
            (balance.total, Decimal::ZERO)
        };

        Self {
            token: balance.coin,
            token_index: balance
                .token
                .expect("borrow/lend positions require a token index"),
            supplied,
            borrowed,
            hold: balance.hold,
        }
    }
}

fn parse_address(input: &str) -> Result<Address, CliError> {
    let stripped = input.strip_prefix("0x").ok_or_else(invalid_address_error)?;
    if stripped.len() != 40 || !stripped.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(invalid_address_error());
    }
    input
        .parse::<Address>()
        .map_err(|_| invalid_address_error())
}

fn invalid_address_error() -> CliError {
    CliError::Configuration("address must be a 0x-prefixed 40-byte hex string".to_string())
}

pub const GROUP: GroupSpec = GroupSpec::new(
    &["borrowlend"],
    "Supply, withdraw, and query borrow/lend reserves",
);

#[derive(Args, Debug, Clone)]
pub struct BorrowlendGetArgs {
    /// Token symbol (e.g., USDC)
    pub token: String,
}

pub struct BorrowlendRates;

impl CliCommand for BorrowlendRates {
    type Args = NoArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["borrowlend", "rates"],
        "market",
        "Show borrow and lend rates",
        "List borrow/lend rates.",
    );

    async fn run(inv: &Invocation, _args: &NoArgs) -> Result<(), anyhow::Error> {
        rates(inv).await
    }
}

pub struct BorrowlendGet;

impl CliCommand for BorrowlendGet {
    type Args = BorrowlendGetArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["borrowlend", "get"],
        "market",
        "Show a token reserve",
        "Show a single borrow/lend token reserve.",
    )
    .args(&[arg("token")
        .kind(InputKind::MarketSymbol)
        .describe("Token symbol, for example USDC.")
        .resource_id("token")]);

    async fn run(inv: &Invocation, args: &BorrowlendGetArgs) -> Result<(), anyhow::Error> {
        get(inv, &args.token).await
    }
}

pub struct BorrowlendUser;

impl CliCommand for BorrowlendUser {
    type Args = OptionalAddressArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["borrowlend", "user"],
        "account",
        "Show borrow and lend state for an address or wallet",
        "Show user borrow/lend state.",
    )
    .args(&[arg("address")
        .kind(InputKind::PublicUserSelector)
        .describe("Ethereum address, wallet name, or wallet id.")
        .resource_id("account selector")
        .schema_required()]);

    async fn run(inv: &Invocation, args: &OptionalAddressArgs) -> Result<(), anyhow::Error> {
        let address = inv.resolve_user(args.address.as_deref())?.to_string();
        user(inv, &address).await
    }
}

pub struct BorrowlendSupply;

impl CliCommand for BorrowlendSupply {
    type Args = ActionArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["borrowlend", "supply"],
        "market",
        "Supply a token to a borrow/lend reserve",
        "Supply a borrow/lend reserve token with a wallet-signed borrowLend exchange action. Runs live without a confirmation prompt; --dry-run previews the action without submitting.",
    )
    .schema_command("hyperliquid borrowlend supply <TOKEN> --amount <AMOUNT>")
    .args(&[
        arg("token").kind(InputKind::MarketSymbol).resource_id("token"),
        arg("amount").kind(InputKind::Amount).describe("Human-readable token amount to supply.").schema_required(),
        arg("max").describe("Not supported for supply: the command rejects --max and requires --amount. Only withdraw accepts --max.").schema_string(),
    ]);

    async fn run(inv: &Invocation, args: &ActionArgs) -> Result<(), anyhow::Error> {
        borrowlend_action(inv, BorrowLendOperation::Supply, args).await
    }
}

pub struct BorrowlendWithdraw;

impl CliCommand for BorrowlendWithdraw {
    type Args = ActionArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["borrowlend", "withdraw"],
        "market",
        "Withdraw a token from a borrow/lend reserve",
        "Withdraw from a borrow/lend reserve with a wallet-signed borrowLend exchange action. Runs live without a confirmation prompt; --dry-run previews the action without submitting.",
    )
    .schema_command("hyperliquid borrowlend withdraw <TOKEN> --amount <AMOUNT>")
    .args(&[
        arg("token").kind(InputKind::MarketSymbol).resource_id("token"),
        arg("amount").kind(InputKind::Amount).describe("Human-readable token amount."),
        arg("max").describe("Withdraw maximum amount; sends amount=null in the exchange borrowLend action.").schema_string(),
    ]);

    async fn run(inv: &Invocation, args: &ActionArgs) -> Result<(), anyhow::Error> {
        borrowlend_action(inv, BorrowLendOperation::Withdraw, args).await
    }
}

async fn borrowlend_action(
    inv: &Invocation,
    operation: BorrowLendOperation,
    args: &ActionArgs,
) -> Result<(), anyhow::Error> {
    let preview = action_preview(inv.remote()?.api(), operation, args).await?;
    if inv.dry_run() {
        let envelope = signed_action_dry_run_envelope(
            inv,
            &format!("borrowlend {}", operation.as_str()),
            &format!("{}_borrowlend", operation.as_str()),
            operation.reversibility(),
            preview,
            default_dry_run_signing_context(inv),
        );
        return print_dry_run_envelope(inv, envelope);
    }

    let resolved = inv.resolve_signer()?;
    let signer = resolved.selected_signer();
    action(inv, &signer, operation, args).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_address_rejects_bad_format_as_usage_error() {
        let err = parse_address("INVALID").unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn decimal_to_wei_respects_token_decimals() {
        assert_eq!(decimal_to_wei(Decimal::new(123, 1), 6).unwrap(), 12_300_000);
        assert!(decimal_to_wei(Decimal::new(1, 7), 6).is_err());
    }

    #[test]
    fn action_args_require_amount_or_max() {
        let err = validate_action_args(
            BorrowLendOperation::Withdraw,
            &ActionArgs {
                token: "USDC".to_string(),
                amount: None,
                max: false,
            },
        )
        .unwrap_err();

        assert_eq!(err.exit_code(), 13);
    }

    #[test]
    fn supply_rejects_max_sentinel() {
        let err = validate_action_args(
            BorrowLendOperation::Supply,
            &ActionArgs {
                token: "USDC".to_string(),
                amount: None,
                max: true,
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("--max is only supported"));
    }

    #[test]
    fn borrowlend_operations_declare_reversibility() {
        assert_eq!(
            BorrowLendOperation::Supply.reversibility(),
            ActionReversibility::PartiallyReversible
        );
        assert_eq!(
            BorrowLendOperation::Withdraw.reversibility(),
            ActionReversibility::PartiallyReversible
        );
    }
}
