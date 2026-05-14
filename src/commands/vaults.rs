//! Vault public queries and authenticated vault transfers.

use std::time::{Duration, Instant};

use clap::{Args, ValueEnum};
use hypersdk::hypercore::api::VaultTransfer;
use hypersdk::hypercore::types::{Action, LeverageType};
use hypersdk::hypercore::{AssetPosition, Chain, HttpClient};
use hypersdk::{Address, Decimal};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{actions, map_api_error};
use crate::dry_run::ActionReversibility;
use crate::errors::CliError;
use crate::http_api::post_info_json;
use crate::output::{self, OutputFormat, TableData};
use crate::signing::SelectedSigner;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultTransferActionKind {
    Deposit,
    Withdraw,
}

impl VaultTransferActionKind {
    #[must_use]
    pub fn reversibility(self) -> ActionReversibility {
        ActionReversibility::PartiallyReversible
    }

    #[must_use]
    pub fn is_deposit(self) -> bool {
        matches!(self, Self::Deposit)
    }

    #[must_use]
    pub fn action_name(self) -> &'static str {
        if self.is_deposit() {
            "deposit"
        } else {
            "withdraw"
        }
    }
}

/// Arguments for `vault deposit` and `vault withdraw`.
#[derive(Args, Debug, Clone)]
pub struct VaultTransferArgs {
    /// Vault address
    #[arg(long)]
    pub vault: String,

    /// Amount of USDC to deposit or withdraw
    #[arg(long, allow_hyphen_values = true)]
    pub amount: Decimal,
}

/// Arguments for `vault list`.
#[derive(Args, Debug, Clone)]
pub struct VaultListArgs {
    /// Filter by vault kind
    #[arg(long, value_enum, default_value = "all")]
    pub kind: VaultKindArg,

    /// Optional user address for user-specific deposit context
    #[arg(long)]
    pub user: Option<String>,

    /// Maximum number of vaults to return
    #[arg(long, default_value_t = 25, value_parser = clap::value_parser!(usize))]
    pub limit: usize,

    /// Sort field
    #[arg(long, value_enum, default_value = "tvl")]
    pub sort: VaultSortArg,
}

/// Arguments for `vault search`.
#[derive(Args, Debug, Clone)]
pub struct VaultSearchArgs {
    /// Search query matched against vault name, leader, or address
    pub query: String,

    /// Optional user address for user-specific deposit context
    #[arg(long)]
    pub user: Option<String>,

    /// Maximum number of vaults to return
    #[arg(long, default_value_t = 25, value_parser = clap::value_parser!(usize))]
    pub limit: usize,

    /// Sort field
    #[arg(long, value_enum, default_value = "tvl")]
    pub sort: VaultSortArg,
}

/// Vault relationship filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum VaultKindArg {
    All,
    Normal,
    Child,
    Parent,
    Protocol,
    User,
}

impl VaultKindArg {
    fn matches(self, kind: &str) -> bool {
        match self {
            Self::All => true,
            Self::Protocol => kind.eq_ignore_ascii_case("normal"),
            Self::User => kind.eq_ignore_ascii_case("child") || kind.eq_ignore_ascii_case("parent"),
            _ => self.to_string().eq_ignore_ascii_case(kind),
        }
    }
}

impl std::fmt::Display for VaultKindArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => write!(f, "all"),
            Self::Normal => write!(f, "normal"),
            Self::Child => write!(f, "child"),
            Self::Parent => write!(f, "parent"),
            Self::Protocol => write!(f, "protocol"),
            Self::User => write!(f, "user"),
        }
    }
}

/// Vault list sort field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum VaultSortArg {
    Tvl,
    Apr,
    Age,
    Name,
}

#[derive(Debug, Clone, Serialize)]
struct VaultDetailsRow {
    name: String,
    vault_address: String,
    leader: String,
    description: String,
    #[serde(with = "rust_decimal::serde::str")]
    tvl: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    apr: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    leader_fraction: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    leader_commission: Decimal,
    followers: usize,
    allow_deposits: bool,
    is_closed: bool,
    #[serde(with = "rust_decimal::serde::str")]
    max_distributable: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    max_withdrawable: Decimal,
    portfolio_periods: Vec<String>,
}

pub struct VaultDetailsOutput {
    row: VaultDetailsRow,
}

#[derive(Debug, Clone, Serialize)]
struct VaultSummaryRow {
    vault_address: String,
    name: String,
    leader: String,
    #[serde(with = "rust_decimal::serde::str")]
    apr: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    tvl: Decimal,
    age_days: Option<i64>,
    kind: String,
    #[serde(with = "rust_decimal::serde::str_option")]
    user_deposit: Option<Decimal>,
}

pub struct VaultSummaryOutput {
    rows: Vec<VaultSummaryRow>,
}

#[derive(Debug, Clone, Serialize)]
struct VaultPositionRow {
    coin: String,
    side: String,
    #[serde(with = "rust_decimal::serde::str")]
    size: Decimal,
    #[serde(with = "rust_decimal::serde::str_option")]
    entry: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str")]
    mark: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    unrealized_pnl: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    position_value: Decimal,
    leverage: String,
    #[serde(with = "rust_decimal::serde::str")]
    margin_used: Decimal,
    liquidation_price: Option<String>,
}

pub struct VaultPositionsOutput {
    vault_address: String,
    rows: Vec<VaultPositionRow>,
}

#[derive(Debug, Clone, Serialize)]
struct VaultTransferConfirmation {
    action: String,
    status: String,
    network: String,
    signer: String,
    acting_as: String,
    reversibility: String,
    vault_address: String,
    #[serde(with = "rust_decimal::serde::str")]
    amount: Decimal,
    asset: String,
}

struct VaultTransferOutput {
    row: VaultTransferConfirmation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VaultQueryResult<T> {
    pub output: T,
    pub elapsed: Duration,
}

impl TableData for VaultDetailsOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Name",
            "Vault",
            "TVL",
            "APR",
            "Strategy",
            "Leader",
            "Followers",
            "Allow Deposits",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.row.name.clone(),
            self.row.vault_address.clone(),
            self.row.tvl.to_string(),
            self.row.apr.to_string(),
            self.row.description.clone(),
            self.row.leader.clone(),
            self.row.followers.to_string(),
            self.row.allow_deposits.to_string(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.row).unwrap_or_else(|_| serde_json::json!({}))
    }
}

impl TableData for VaultSummaryOutput {
    fn headers(&self) -> Vec<&str> {
        if self.rows.is_empty() {
            vec!["Message"]
        } else {
            let mut headers = vec!["Vault", "Name", "Leader", "APR", "TVL", "Age Days", "Kind"];
            if self.rows.iter().any(|row| row.user_deposit.is_some()) {
                headers.push("User Deposit");
            }
            headers
        }
    }

    fn rows(&self) -> Vec<Vec<String>> {
        if self.rows.is_empty() {
            return vec![vec!["No vaults found".to_string()]];
        }
        let show_user_deposit = self.rows.iter().any(|row| row.user_deposit.is_some());
        self.rows
            .iter()
            .map(|row| {
                let mut columns = vec![
                    row.vault_address.clone(),
                    row.name.clone(),
                    row.leader.clone(),
                    row.apr.to_string(),
                    row.tvl.to_string(),
                    row.age_days.map(|age| age.to_string()).unwrap_or_default(),
                    row.kind.clone(),
                ];
                if show_user_deposit {
                    columns.push(
                        row.user_deposit
                            .map(|deposit| deposit.to_string())
                            .unwrap_or_default(),
                    );
                }
                columns
            })
            .collect()
    }

    fn pretty_rows(&self) -> Vec<Vec<String>> {
        if self.rows.is_empty() {
            return vec![vec![output::colors::gray("No vaults found")]];
        }
        self.rows()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

impl TableData for VaultPositionsOutput {
    fn headers(&self) -> Vec<&str> {
        if self.rows.is_empty() {
            vec!["Vault", "Message"]
        } else {
            vec![
                "Vault",
                "Coin",
                "Side",
                "Size",
                "Entry",
                "Mark",
                "Unrealized PnL",
                "Position Value",
                "Leverage",
                "Margin Used",
                "Liquidation",
            ]
        }
    }

    fn rows(&self) -> Vec<Vec<String>> {
        if self.rows.is_empty() {
            return vec![vec![
                self.vault_address.clone(),
                "No open positions".to_string(),
            ]];
        }

        self.rows
            .iter()
            .map(|row| {
                vec![
                    self.vault_address.clone(),
                    row.coin.clone(),
                    row.side.clone(),
                    row.size.to_string(),
                    row.entry
                        .map(|entry| entry.to_string())
                        .unwrap_or_else(|| "n/a".to_string()),
                    row.mark.to_string(),
                    row.unrealized_pnl.to_string(),
                    row.position_value.to_string(),
                    row.leverage.clone(),
                    row.margin_used.to_string(),
                    row.liquidation_price
                        .clone()
                        .unwrap_or_else(|| "n/a".to_string()),
                ]
            })
            .collect()
    }

    fn pretty_rows(&self) -> Vec<Vec<String>> {
        self.rows()
            .into_iter()
            .map(|mut row| {
                if self.rows.is_empty() {
                    row[1] = output::colors::gray(&row[1]);
                    return row;
                }
                if row.len() > 6
                    && let Some(position) =
                        self.rows.iter().find(|position| position.coin == row[1])
                {
                    if position.unrealized_pnl > Decimal::ZERO {
                        row[6] = output::colors::green(&row[6]);
                    } else if position.unrealized_pnl < Decimal::ZERO {
                        row[6] = output::colors::red(&row[6]);
                    }
                }
                for cell in &mut row {
                    if cell == "n/a" {
                        *cell = output::colors::gray(cell);
                    }
                }
                row
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "vault_address": self.vault_address,
            "positions": self.rows,
        })
    }
}

impl TableData for VaultTransferOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Action",
            "Status",
            "Network",
            "Signer",
            "Acting As",
            "Reversibility",
            "Vault",
            "Amount",
            "Asset",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.row.action.clone(),
            self.row.status.clone(),
            self.row.network.clone(),
            self.row.signer.clone(),
            self.row.acting_as.clone(),
            self.row.reversibility.clone(),
            self.row.vault_address.clone(),
            self.row.amount.to_string(),
            self.row.asset.clone(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.row).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// Get vault details.
pub async fn get(
    api_base_url: &str,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = get_query(api_base_url, address).await?;
    output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Get vault details without printing them.
pub async fn get_query(
    api_base_url: &str,
    address: &str,
) -> Result<VaultQueryResult<VaultDetailsOutput>, anyhow::Error> {
    let start = Instant::now();
    let vault = parse_address(address)?;
    let row = raw_vault_details_row(api_base_url, vault).await?;
    Ok(VaultQueryResult {
        output: VaultDetailsOutput { row },
        elapsed: start.elapsed(),
    })
}

/// List vault summaries.
pub async fn list(
    api_base_url: &str,
    args: &VaultListArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = list_query(api_base_url, args).await?;
    output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// List vault summaries without printing them.
pub async fn list_query(
    api_base_url: &str,
    args: &VaultListArgs,
) -> Result<VaultQueryResult<VaultSummaryOutput>, anyhow::Error> {
    let start = Instant::now();
    let user = validate_list_args(args)?;
    let mut rows = vault_summary_rows(api_base_url, user).await?;
    rows.retain(|row| args.kind.matches(&row.kind));
    sort_vault_rows(&mut rows, args.sort);
    rows.truncate(args.limit);
    Ok(VaultQueryResult {
        output: VaultSummaryOutput { rows },
        elapsed: start.elapsed(),
    })
}

/// Search vault summaries by name, leader, or address.
pub async fn search(
    api_base_url: &str,
    args: &VaultSearchArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = search_query(api_base_url, args).await?;
    output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Search vault summaries without printing them.
pub async fn search_query(
    api_base_url: &str,
    args: &VaultSearchArgs,
) -> Result<VaultQueryResult<VaultSummaryOutput>, anyhow::Error> {
    let start = Instant::now();
    let user = validate_search_args(args)?;
    let query = args.query.to_ascii_lowercase();
    let mut rows = vault_summary_rows(api_base_url, user)
        .await?
        .into_iter()
        .filter(|row| {
            row.name.to_ascii_lowercase().contains(&query)
                || row.leader.to_ascii_lowercase().contains(&query)
                || row.vault_address.to_ascii_lowercase().contains(&query)
        })
        .collect::<Vec<_>>();
    sort_vault_rows(&mut rows, args.sort);
    rows.truncate(args.limit);
    Ok(VaultQueryResult {
        output: VaultSummaryOutput { rows },
        elapsed: start.elapsed(),
    })
}

/// List a vault's open positions.
pub async fn positions(
    client: &HttpClient,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = positions_query(client, address).await?;
    output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// List a vault's open positions without printing them.
pub async fn positions_query(
    client: &HttpClient,
    address: &str,
) -> Result<VaultQueryResult<VaultPositionsOutput>, anyhow::Error> {
    let start = Instant::now();
    let vault = parse_address(address)?;
    let rows = client
        .clearinghouse_state(vault, None)
        .await
        .map_err(map_api_error)?
        .asset_positions
        .into_iter()
        .map(VaultPositionRow::from)
        .filter(|row| !row.size.is_zero())
        .collect::<Vec<_>>();

    Ok(VaultQueryResult {
        output: VaultPositionsOutput {
            vault_address: address.to_string(),
            rows,
        },
        elapsed: start.elapsed(),
    })
}

/// Deposit USDC to a vault.
pub async fn deposit(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    args: &VaultTransferArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    transfer(
        api_base_url,
        chain,
        signer,
        args,
        VaultTransferActionKind::Deposit,
        format,
    )
    .await
}

/// Withdraw USDC from a vault.
pub async fn withdraw(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    args: &VaultTransferArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    transfer(
        api_base_url,
        chain,
        signer,
        args,
        VaultTransferActionKind::Withdraw,
        format,
    )
    .await
}

pub fn validate_transfer_args(args: &VaultTransferArgs) -> Result<(), CliError> {
    parse_address(&args.vault)?;
    validate_usdc_amount(args.amount)?;
    Ok(())
}

pub fn transfer_dry_run_value(
    chain: Chain,
    args: &VaultTransferArgs,
    action_kind: VaultTransferActionKind,
) -> Result<serde_json::Value, CliError> {
    let vault = parse_address(&args.vault)?;
    let usd = validate_usdc_amount(args.amount)?;
    Ok(serde_json::json!({
        "action": action_kind.action_name(),
        "network": chain.to_string(),
        "vault": vault.to_string(),
        "amount": args.amount.to_string(),
        "asset": "USDC",
        "is_deposit": action_kind.is_deposit(),
        "usd": usd,
        "reversibility": action_kind.reversibility(),
        "verified_shape": {
            "type": "vaultTransfer",
            "vaultAddress": vault.to_string(),
            "isDeposit": action_kind.is_deposit(),
            "usd": usd
        }
    }))
}

pub fn validate_list_args(args: &VaultListArgs) -> Result<Option<Address>, CliError> {
    if args.limit == 0 {
        return Err(CliError::Configuration(
            "vault list --limit must be greater than zero".to_string(),
        ));
    }
    parse_optional_user(args.user.as_deref())
}

pub fn parse_kind_arg(value: &str) -> Result<VaultKindArg, CliError> {
    match value.to_ascii_lowercase().as_str() {
        "all" => Ok(VaultKindArg::All),
        "normal" => Ok(VaultKindArg::Normal),
        "child" => Ok(VaultKindArg::Child),
        "parent" => Ok(VaultKindArg::Parent),
        "protocol" => Ok(VaultKindArg::Protocol),
        "user" => Ok(VaultKindArg::User),
        _ => Err(CliError::Configuration(format!(
            "vault kind must be one of all, normal, child, parent, protocol, user: {value}"
        ))),
    }
}

pub fn parse_sort_arg(value: &str) -> Result<VaultSortArg, CliError> {
    match value.to_ascii_lowercase().as_str() {
        "tvl" => Ok(VaultSortArg::Tvl),
        "apr" => Ok(VaultSortArg::Apr),
        "age" => Ok(VaultSortArg::Age),
        "name" => Ok(VaultSortArg::Name),
        _ => Err(CliError::Configuration(format!(
            "vault sort must be one of tvl, apr, age, name: {value}"
        ))),
    }
}

pub fn validate_search_args(args: &VaultSearchArgs) -> Result<Option<Address>, CliError> {
    if args.query.trim().is_empty() {
        return Err(CliError::Configuration(
            "vault search query cannot be empty".to_string(),
        ));
    }
    if args.limit == 0 {
        return Err(CliError::Configuration(
            "vault search --limit must be greater than zero".to_string(),
        ));
    }
    parse_optional_user(args.user.as_deref())
}

async fn transfer(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    args: &VaultTransferArgs,
    action_kind: VaultTransferActionKind,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let vault = parse_address(&args.vault)?;
    let usd = validate_usdc_amount(args.amount)?;
    let nonce = actions::nonce_now();
    let hl_action = Action::VaultTransfer(VaultTransfer {
        vault_address: vault,
        is_deposit: action_kind.is_deposit(),
        usd,
    });

    actions::send_l1_action(api_base_url, chain, signer, hl_action, nonce).await?;

    output::print_data(
        &VaultTransferOutput {
            row: VaultTransferConfirmation {
                action: action_kind.action_name().to_string(),
                status: "submitted".to_string(),
                network: chain.to_string(),
                signer: signer.address().to_string(),
                acting_as: signer.query_address().to_string(),
                reversibility: format_reversibility(action_kind.reversibility()).to_string(),
                vault_address: args.vault.clone(),
                amount: args.amount,
                asset: "USDC".to_string(),
            },
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RawVaultDetailsRequest {
    #[serde(rename = "type")]
    request_type: &'static str,
    vault_address: Address,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RawVaultSummariesRequest {
    #[serde(rename = "type")]
    request_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<Address>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawVaultSummary {
    name: String,
    vault_address: String,
    leader: String,
    tvl: Value,
    #[serde(default)]
    apr: Value,
    is_closed: bool,
    #[serde(default)]
    relationship: Value,
    #[serde(default)]
    create_time_millis: Option<i64>,
    #[serde(default, alias = "userDeposit", alias = "yourDeposit")]
    user_deposit: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawVaultDetails {
    name: String,
    #[serde(default)]
    vault_address: Option<String>,
    leader: String,
    description: String,
    #[serde(default)]
    portfolio: Value,
    apr: Value,
    leader_fraction: Value,
    leader_commission: Value,
    #[serde(default)]
    followers: Value,
    #[serde(default)]
    allow_deposits: bool,
    #[serde(default)]
    is_closed: bool,
    max_distributable: Value,
    max_withdrawable: Value,
}

async fn raw_vault_details_row(
    api_base_url: &str,
    vault: Address,
) -> Result<VaultDetailsRow, CliError> {
    let details: RawVaultDetails = match post_info_json(
        api_base_url,
        &RawVaultDetailsRequest {
            request_type: "vaultDetails",
            vault_address: vault,
        },
        "fetching vault details",
    )
    .await
    {
        Ok(details) => details,
        Err(err) if is_null_object_decode(&err) => {
            return Err(CliError::Unsupported(format!(
                "vault details not found for {}",
                vault
            )));
        }
        Err(err) => return Err(err),
    };
    details.into_row(vault)
}

async fn vault_summary_rows(
    api_base_url: &str,
    user: Option<Address>,
) -> Result<Vec<VaultSummaryRow>, CliError> {
    let summaries: Vec<RawVaultSummary> = post_info_json(
        api_base_url,
        &RawVaultSummariesRequest {
            request_type: "vaultSummaries",
            user,
        },
        "fetching vault summaries",
    )
    .await?;
    summaries
        .into_iter()
        .map(RawVaultSummary::into_row)
        .collect()
}

impl RawVaultSummary {
    fn into_row(self) -> Result<VaultSummaryRow, CliError> {
        let kind = self
            .relationship
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(if self.is_closed { "closed" } else { "normal" })
            .to_string();
        Ok(VaultSummaryRow {
            vault_address: self.vault_address,
            name: self.name,
            leader: self.leader,
            apr: optional_decimal_field(&self.apr).unwrap_or(Decimal::ZERO),
            tvl: decimal_field(&self.tvl, "tvl")?,
            age_days: self.create_time_millis.map(age_days_from_millis),
            kind,
            user_deposit: self.user_deposit.as_ref().and_then(optional_decimal_field),
        })
    }
}

fn parse_optional_user(raw: Option<&str>) -> Result<Option<Address>, CliError> {
    raw.map(parse_address).transpose()
}

fn sort_vault_rows(rows: &mut [VaultSummaryRow], sort: VaultSortArg) {
    match sort {
        VaultSortArg::Tvl => rows.sort_by(|left, right| {
            right
                .tvl
                .cmp(&left.tvl)
                .then_with(|| left.name.cmp(&right.name))
        }),
        VaultSortArg::Apr => rows.sort_by(|left, right| {
            right
                .apr
                .cmp(&left.apr)
                .then_with(|| left.name.cmp(&right.name))
        }),
        VaultSortArg::Age => rows.sort_by(|left, right| {
            right
                .age_days
                .cmp(&left.age_days)
                .then_with(|| left.name.cmp(&right.name))
        }),
        VaultSortArg::Name => rows.sort_by(|left, right| left.name.cmp(&right.name)),
    }
}

fn optional_decimal_field(value: &Value) -> Option<Decimal> {
    match value {
        Value::Null => None,
        Value::String(text) if text.is_empty() => None,
        Value::String(text) => parse_decimal_text(text).ok(),
        Value::Number(number) => parse_decimal_text(&number.to_string()).ok(),
        _ => None,
    }
}

fn age_days_from_millis(create_time_millis: i64) -> i64 {
    let now = chrono::Utc::now().timestamp_millis();
    ((now - create_time_millis).max(0)) / 86_400_000
}

impl RawVaultDetails {
    fn into_row(self, requested_vault: Address) -> Result<VaultDetailsRow, CliError> {
        let max_distributable = decimal_field(&self.max_distributable, "maxDistributable")?;
        let (portfolio_periods, tvl) = raw_portfolio_summary(&self.portfolio);
        Ok(VaultDetailsRow {
            name: self.name,
            vault_address: self
                .vault_address
                .unwrap_or_else(|| requested_vault.to_string()),
            leader: self.leader,
            description: self.description,
            tvl: tvl.unwrap_or(max_distributable),
            apr: decimal_field(&self.apr, "apr")?,
            leader_fraction: decimal_field(&self.leader_fraction, "leaderFraction")?,
            leader_commission: decimal_field(&self.leader_commission, "leaderCommission")?,
            followers: self.followers.as_array().map_or(0, Vec::len),
            allow_deposits: self.allow_deposits,
            is_closed: self.is_closed,
            max_distributable,
            max_withdrawable: decimal_field(&self.max_withdrawable, "maxWithdrawable")?,
            portfolio_periods,
        })
    }
}

fn decimal_field(value: &Value, field: &'static str) -> Result<Decimal, CliError> {
    let raw = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => {
            return Err(CliError::Internal(anyhow::anyhow!(
                "decode failed while reading {field}: expected string or number"
            )));
        }
    };
    parse_decimal_text(&raw).map_err(|err| {
        CliError::Internal(anyhow::anyhow!(
            "decode failed while reading {field}: {err}"
        ))
    })
}

fn parse_decimal_text(raw: &str) -> Result<Decimal, rust_decimal::Error> {
    raw.parse::<Decimal>()
        .or_else(|_| Decimal::from_scientific(raw))
}

fn is_null_object_decode(err: &CliError) -> bool {
    let message = err.to_string();
    message.contains("invalid type: null")
        && message.contains("expected struct RawVaultDetails")
        && message.contains("body=[untrusted remote data] null")
}

fn raw_portfolio_summary(portfolio: &Value) -> (Vec<String>, Option<Decimal>) {
    let mut periods = Vec::new();
    let mut latest_tvl = None;
    let Some(entries) = portfolio.as_array() else {
        return (periods, latest_tvl);
    };

    for entry in entries {
        let Some(pair) = entry.as_array() else {
            continue;
        };
        let Some(period) = pair.first().and_then(Value::as_str) else {
            continue;
        };
        periods.push(period.to_string());

        let Some(history) = pair
            .get(1)
            .and_then(|details| details.get("accountValueHistory"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        let Some(value) = history
            .last()
            .and_then(Value::as_array)
            .and_then(|point| point.get(1))
        else {
            continue;
        };
        if let Ok(decimal) = decimal_field(value, "accountValueHistory") {
            latest_tvl = Some(decimal);
        }
    }

    (periods, latest_tvl)
}

impl From<AssetPosition> for VaultPositionRow {
    fn from(value: AssetPosition) -> Self {
        let position = value.position;
        let abs_size = position.abs_size();
        let mark = if abs_size.is_zero() {
            Decimal::ZERO
        } else {
            position.position_value / abs_size
        };
        let leverage_mode = match position.leverage.leverage_type {
            LeverageType::Cross => "cross",
            LeverageType::Isolated => "isolated",
        };
        let side = position.side().to_string();

        Self {
            coin: position.coin,
            side,
            size: position.szi,
            entry: position.entry_px,
            mark,
            unrealized_pnl: position.unrealized_pnl,
            position_value: position.position_value,
            leverage: format!("{leverage_mode} {}x", position.leverage.value),
            margin_used: position.margin_used,
            liquidation_price: position.liquidation_px.map(|px| px.to_string()),
        }
    }
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

fn format_reversibility(reversibility: ActionReversibility) -> &'static str {
    match reversibility {
        ActionReversibility::Reversible => "reversible",
        ActionReversibility::PartiallyReversible => "partially_reversible",
        ActionReversibility::Irreversible => "irreversible",
    }
}

fn validate_usdc_amount(amount: Decimal) -> Result<u64, CliError> {
    if amount <= Decimal::ZERO {
        return Err(CliError::Configuration(
            "amount must be greater than zero".to_string(),
        ));
    }
    let micro_units = amount * Decimal::from(1_000_000_u64);
    if micro_units.fract() != Decimal::ZERO {
        return Err(CliError::Configuration(
            "amount supports at most 6 decimal places".to_string(),
        ));
    }
    micro_units
        .to_u64()
        .ok_or_else(|| CliError::Configuration("amount is too large".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usdc_amount_validation_rejects_zero_negative_and_over_precise_amounts() {
        assert!(validate_usdc_amount(Decimal::ZERO).is_err());
        assert!(validate_usdc_amount("-1".parse::<Decimal>().unwrap()).is_err());
        assert!(validate_usdc_amount("0.0000001".parse::<Decimal>().unwrap()).is_err());
        assert_eq!(
            validate_usdc_amount("1.5".parse::<Decimal>().unwrap()).unwrap(),
            1_500_000
        );
    }

    #[test]
    fn parse_address_rejects_bad_and_zero_addresses() {
        assert_eq!(parse_address("INVALID").unwrap_err().exit_code(), 2);
        assert_eq!(
            parse_address("0x0000000000000000000000000000000000000000")
                .unwrap_err()
                .exit_code(),
            2
        );
    }

    #[test]
    fn vault_transfer_actions_declare_reversibility() {
        assert_eq!(
            VaultTransferActionKind::Deposit.reversibility(),
            ActionReversibility::PartiallyReversible
        );
        assert_eq!(
            VaultTransferActionKind::Withdraw.reversibility(),
            ActionReversibility::PartiallyReversible
        );
    }
}
