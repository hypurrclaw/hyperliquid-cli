//! Borrow/lend public market data commands.

use std::time::{Duration, Instant};

use clap::Args;
use hypersdk::hypercore::{Chain, HttpClient, UserBalance};
use hypersdk::{Address, Decimal};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use crate::commands::spot_balances::{RawSpotBalance, user_spot_balances_raw_from_url};
use crate::commands::{actions, map_api_error, raw_info_base_url};
use crate::dry_run::ActionReversibility;
use crate::errors::{CliError, http_response_indicates_rate_limit};
use crate::http_api::{decode_json, ensure_success_response, post_info_raw};
use crate::output::{self, OutputFormat, TableData};
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

#[derive(Debug, Clone, PartialEq)]
pub struct BorrowLendQueryResult<T> {
    pub output: T,
    pub elapsed: Duration,
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
        if self.rows.is_empty() {
            vec!["Message"]
        } else {
            vec![
                "Token",
                "Supply Rate",
                "Borrow Rate",
                "Total Supply",
                "Total Borrow",
                "Note",
            ]
        }
    }

    fn rows(&self) -> Vec<Vec<String>> {
        if self.rows.is_empty() {
            return vec![vec!["No reserve states returned".to_string()]];
        }

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
                if self.rows.is_empty() {
                    row[0] = output::colors::gray(&row[0]);
                } else if row.len() > 5 {
                    row[5] = output::colors::gray(&row[5]);
                }
                row
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

impl TableData for UserBorrowLendOutput {
    fn headers(&self) -> Vec<&str> {
        if self.positions.is_empty() {
            vec!["User", "Message"]
        } else {
            vec!["User", "Token", "Supplied", "Borrowed", "Hold"]
        }
    }

    fn rows(&self) -> Vec<Vec<String>> {
        if self.positions.is_empty() {
            return vec![vec![
                self.user.clone(),
                "No borrow/lend positions".to_string(),
            ]];
        }

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

    fn pretty_rows(&self) -> Vec<Vec<String>> {
        self.rows()
            .into_iter()
            .map(|mut row| {
                if self.positions.is_empty() && row.len() > 1 {
                    row[1] = output::colors::gray(&row[1]);
                }
                row
            })
            .collect()
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
pub async fn rates(
    client: &HttpClient,
    api_base_url: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = rates_query(client, api_base_url).await?;
    output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Fetch reserve states for all borrow/lend tokens without printing them.
pub async fn rates_query(
    client: &HttpClient,
    api_base_url: &str,
) -> Result<BorrowLendQueryResult<ReserveOutput>, anyhow::Error> {
    let start = Instant::now();
    let rows = reserve_rows(client, api_base_url).await?;
    Ok(BorrowLendQueryResult {
        output: ReserveOutput { rows },
        elapsed: start.elapsed(),
    })
}

/// Show reserve state for a single token.
pub async fn get(
    client: &HttpClient,
    api_base_url: &str,
    token: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = get_query(client, api_base_url, token).await?;
    output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Fetch reserve state for a single token without printing it.
pub async fn get_query(
    client: &HttpClient,
    api_base_url: &str,
    token: &str,
) -> Result<BorrowLendQueryResult<ReserveOutput>, anyhow::Error> {
    let start = Instant::now();
    let token = resolve_spot_token(client, token).await?;
    let row = match live_reserve_row(api_base_url, token.index, token.name.clone()).await {
        Ok(row) => row,
        Err(BorrowLendInfoError::EndpointUnavailable) => unavailable_reserve_row(token),
        Err(BorrowLendInfoError::Cli(err)) => return Err(err.into()),
    };

    Ok(BorrowLendQueryResult {
        output: ReserveOutput { rows: vec![row] },
        elapsed: start.elapsed(),
    })
}

/// Show public borrow/lend state for a user.
pub async fn user(
    client: &HttpClient,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = user_query(client, address).await?;
    output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Fetch public borrow/lend state for a user without printing it.
pub async fn user_query(
    client: &HttpClient,
    address: &str,
) -> Result<BorrowLendQueryResult<UserBorrowLendOutput>, anyhow::Error> {
    let api_url = raw_info_base_url(client)?;
    user_query_with_api_base_url(api_url.as_str(), address).await
}

/// Fetch public borrow/lend state for a user from a specific API base URL.
pub async fn user_query_with_api_base_url(
    api_base_url: &str,
    address: &str,
) -> Result<BorrowLendQueryResult<UserBorrowLendOutput>, anyhow::Error> {
    let start = Instant::now();
    let user = parse_address(address)?;
    let balances = user_spot_balances_raw_from_url(api_base_url, user).await?;
    let positions = balances
        .into_iter()
        .filter(|balance| balance.token.is_some())
        .filter(|balance| !balance.total.is_zero() || !balance.hold.is_zero())
        .map(UserBorrowLendPosition::from)
        .collect::<Vec<_>>();

    Ok(BorrowLendQueryResult {
        output: UserBorrowLendOutput {
            user: address.to_string(),
            positions,
        },
        elapsed: start.elapsed(),
    })
}

pub async fn action_preview(
    client: &HttpClient,
    chain: Chain,
    operation: BorrowLendOperation,
    args: &ActionArgs,
) -> Result<serde_json::Value, CliError> {
    validate_action_args(operation, args)?;
    let token = resolve_spot_token(client, &args.token).await?;
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
    client: &HttpClient,
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    operation: BorrowLendOperation,
    args: &ActionArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
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
        api_base_url,
        chain,
        signer,
        &action,
        actions::RawL1ActionMetadata::new(actions::nonce_now()),
        "borrow/lend action failed",
    )
    .await?;

    output::print_data(
        &BorrowLendActionOutput {
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
        },
        format,
        start.elapsed(),
    );
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

async fn reserve_rows(
    client: &HttpClient,
    api_base_url: &str,
) -> Result<Vec<ReserveRow>, CliError> {
    let mut rows = match live_reserve_rows(client, api_base_url).await {
        Ok(rows) => rows,
        Err(BorrowLendInfoError::EndpointUnavailable) => unavailable_reserve_rows(client).await?,
        Err(BorrowLendInfoError::Cli(err)) => return Err(err),
    };

    rows.sort_by(|left, right| {
        (left.token != "USDC")
            .cmp(&(right.token != "USDC"))
            .then_with(|| left.token.cmp(&right.token))
    });

    Ok(rows)
}

async fn live_reserve_rows(
    client: &HttpClient,
    api_base_url: &str,
) -> Result<Vec<ReserveRow>, BorrowLendInfoError> {
    let reserve_states = post_info::<Vec<(u32, RawReserveState)>>(
        api_base_url,
        &BorrowLendInfoRequest::AllReserveStates,
    )
    .await?;
    let token_names = token_names_by_index(client).await?;

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
    api_base_url: &str,
    token_index: u32,
    token_name: String,
) -> Result<ReserveRow, BorrowLendInfoError> {
    let reserve_state = post_info::<RawReserveState>(
        api_base_url,
        &BorrowLendInfoRequest::ReserveState { token: token_index },
    )
    .await?;
    Ok(reserve_state.to_row(token_name, token_index))
}

async fn unavailable_reserve_rows(client: &HttpClient) -> Result<Vec<ReserveRow>, CliError> {
    Ok(client
        .spot_tokens()
        .await
        .map_err(map_api_error)?
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
        .map_err(map_api_error)
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
        .map_err(map_api_error)?
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
    api_base_url: &str,
    request: &BorrowLendInfoRequest,
) -> Result<T, BorrowLendInfoError> {
    let response = post_info_raw(api_base_url, request).await?;
    let status = response.status;
    let body = response.body;

    if http_response_indicates_rate_limit(status.as_u16(), &body) {
        return Err(CliError::RateLimited.into());
    }

    if info_request_type_is_unavailable(status.as_u16(), &body) {
        return Err(BorrowLendInfoError::EndpointUnavailable);
    }

    ensure_success_response(status, &body)?;

    decode_json::<T>(&body, "borrow/lend").map_err(Into::into)
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

impl From<UserBalance> for UserBorrowLendPosition {
    fn from(balance: UserBalance) -> Self {
        let (supplied, borrowed) = if balance.total < Decimal::ZERO {
            (Decimal::ZERO, -balance.total)
        } else {
            (balance.total, Decimal::ZERO)
        };

        Self {
            token: balance.coin,
            token_index: balance.token,
            supplied,
            borrowed,
            hold: balance.hold,
        }
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
