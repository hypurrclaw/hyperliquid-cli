//! Authenticated position management commands.

use clap::Args;
use hypersdk::Address;
use hypersdk::Decimal;
use hypersdk::hypercore::HttpClient;
use hypersdk::hypercore::Subscription;
use hypersdk::hypercore::api::UpdateLeverage;
use hypersdk::hypercore::types::api::UpdateIsolatedMargin;
use hypersdk::hypercore::types::{Action, AssetPosition, LeverageType};

use crate::command_spec::{
    CliCommand, CommandSpec, GroupSpec, InputKind, OwsSupport, Risk, StreamBound, arg,
};
use crate::commands::support::{dry_run_signed_details, dry_run_signing_addresses, print_dry_run};
use crate::signing::SelectedSigner;
use crate::stream::SnapshotWatchArgs;
use serde::Serialize;

use crate::assets::AssetResolver;
use crate::commands::actions;
use crate::errors::CliError;
use crate::errors::from_sdk;
use crate::invocation::Invocation;
use crate::output::{self, TableData};

/// Arguments for `positions update-leverage`.
#[derive(Args, Debug, Clone)]
pub struct UpdateLeverageArgs {
    /// Perpetual coin (for example: BTC, ETH, SOL)
    #[arg(long)]
    pub coin: String,

    /// New leverage value
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=100))]
    pub leverage: u32,

    /// Use isolated margin mode instead of cross margin
    #[arg(long)]
    pub isolated: bool,
}

/// Arguments for `positions update-margin`.
#[derive(Args, Debug, Clone)]
pub struct UpdateMarginArgs {
    /// Perpetual coin (for example: BTC, ETH, SOL)
    #[arg(long)]
    pub coin: String,

    /// Amount of isolated margin to add, in USDC
    #[arg(long, allow_hyphen_values = true)]
    pub amount: Decimal,
}

#[derive(Debug, Clone, Serialize)]
struct PositionRow {
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

impl From<AssetPosition> for PositionRow {
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

pub struct PositionsOutput {
    rows: Vec<PositionRow>,
}

impl TableData for PositionsOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
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

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
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
                if row.len() > 5 {
                    let pnl = self
                        .rows
                        .iter()
                        .find(|position| position.coin == row[0])
                        .map(|position| position.unrealized_pnl)
                        .unwrap_or_default();
                    if pnl > Decimal::ZERO {
                        row[5] = output::colors::green(&row[5]);
                    } else if pnl < Decimal::ZERO {
                        row[5] = output::colors::red(&row[5]);
                    }
                }
                for cell in row.iter_mut() {
                    if cell == "n/a" {
                        *cell = output::colors::gray(cell);
                    }
                }
                row
            })
            .collect()
    }

    fn empty_message(&self) -> Option<&str> {
        Some("No open positions.")
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize)]
struct PositionActionConfirmation {
    coin: String,
    action: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    leverage: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    margin_mode: Option<String>,
    #[serde(with = "rust_decimal::serde::str_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    amount: Option<Decimal>,
}

struct PositionActionOutput {
    rows: Vec<PositionActionConfirmation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedUpdateLeverage {
    coin: String,
    asset: usize,
    is_cross: bool,
    leverage: u32,
}

impl PreparedUpdateLeverage {
    #[must_use]
    pub fn coin(&self) -> &str {
        &self.coin
    }

    #[must_use]
    pub fn asset(&self) -> usize {
        self.asset
    }

    #[must_use]
    pub fn is_cross(&self) -> bool {
        self.is_cross
    }

    #[must_use]
    pub fn leverage(&self) -> u32 {
        self.leverage
    }

    #[must_use]
    pub fn margin_mode(&self) -> &'static str {
        if self.is_cross { "cross" } else { "isolated" }
    }

    fn action(&self) -> Action {
        Action::UpdateLeverage(UpdateLeverage {
            asset: self.asset,
            is_cross: self.is_cross,
            leverage: self.leverage,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedUpdateMargin {
    coin: String,
    asset: usize,
    amount: Decimal,
    ntli: i64,
}

impl PreparedUpdateMargin {
    #[must_use]
    pub fn coin(&self) -> &str {
        &self.coin
    }

    #[must_use]
    pub fn asset(&self) -> usize {
        self.asset
    }

    #[must_use]
    pub fn amount(&self) -> Decimal {
        self.amount
    }

    #[must_use]
    pub fn ntli(&self) -> i64 {
        self.ntli
    }

    fn action(&self) -> Action {
        Action::UpdateIsolatedMargin(UpdateIsolatedMargin {
            asset: self.asset,
            is_buy: true,
            ntli: self.ntli,
        })
    }
}

#[must_use]
pub fn update_leverage_dry_run_value(
    network: impl Into<String>,
    args: &UpdateLeverageArgs,
    prepared: &PreparedUpdateLeverage,
) -> serde_json::Value {
    serde_json::json!({
        "coin": args.coin,
        "resolved_coin": prepared.coin(),
        "asset": prepared.asset(),
        "network": network.into(),
        "leverage": prepared.leverage(),
        "margin_mode": prepared.margin_mode(),
        "is_cross": prepared.is_cross(),
    })
}

#[must_use]
pub fn update_margin_dry_run_value(
    network: impl Into<String>,
    args: &UpdateMarginArgs,
    prepared: &PreparedUpdateMargin,
) -> serde_json::Value {
    serde_json::json!({
        "coin": args.coin,
        "resolved_coin": prepared.coin(),
        "asset": prepared.asset(),
        "network": network.into(),
        "amount": prepared.amount().to_string(),
        "ntli": prepared.ntli(),
        "margin_mode": "isolated",
    })
}

impl TableData for PositionActionOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Coin",
            "Action",
            "Status",
            "Leverage",
            "Margin Mode",
            "Amount",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.coin.clone(),
                    row.action.clone(),
                    row.status.clone(),
                    row.leverage
                        .map(|leverage| leverage.to_string())
                        .unwrap_or_default(),
                    row.margin_mode.clone().unwrap_or_default(),
                    row.amount
                        .map(|amount| amount.to_string())
                        .unwrap_or_default(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

/// List authenticated user's open positions.
pub async fn list(inv: &Invocation, user: Address) -> Result<(), anyhow::Error> {
    inv.print(&list_snapshot(inv.remote()?.api().sdk(), user).await?);
    Ok(())
}

/// Fetch authenticated user's open positions without printing them.
pub async fn list_snapshot(
    client: &HttpClient,
    user: Address,
) -> Result<PositionsOutput, anyhow::Error> {
    let rows = client
        .clearinghouse_state(user, None)
        .await
        .map_err(from_sdk)?
        .asset_positions
        .into_iter()
        .map(PositionRow::from)
        .filter(|row| !row.size.is_zero())
        .collect::<Vec<_>>();

    Ok(PositionsOutput { rows })
}

pub fn prepare_update_leverage(
    resolver: &AssetResolver,
    args: &UpdateLeverageArgs,
) -> Result<PreparedUpdateLeverage, CliError> {
    validate_update_leverage_args(args)?;
    let (coin, asset) = resolve_perp_asset(resolver, &args.coin)?;
    Ok(PreparedUpdateLeverage {
        coin,
        asset,
        is_cross: !args.isolated,
        leverage: args.leverage,
    })
}

pub async fn submit_update_leverage(
    inv: &Invocation,
    signer: &SelectedSigner,
    prepared: PreparedUpdateLeverage,
) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    actions::send_l1_action(
        remote.api(),
        signer,
        prepared.action(),
        actions::nonce_now(),
    )
    .await?;
    let margin_mode = prepared.margin_mode().to_string();
    let leverage = prepared.leverage;
    let coin = prepared.coin;

    inv.print(&PositionActionOutput {
        rows: vec![PositionActionConfirmation {
            coin,
            action: "update-leverage".to_string(),
            status: "updated".to_string(),
            leverage: Some(leverage),
            margin_mode: Some(margin_mode),
            amount: None,
        }],
    });
    Ok(())
}

pub fn prepare_update_margin(
    resolver: &AssetResolver,
    args: &UpdateMarginArgs,
) -> Result<PreparedUpdateMargin, CliError> {
    let ntli = usd_to_micro_units(args.amount, "amount")?;
    let (coin, asset) = resolve_perp_asset(resolver, &args.coin)?;
    Ok(PreparedUpdateMargin {
        coin,
        asset,
        amount: args.amount,
        ntli,
    })
}

pub async fn submit_update_margin(
    inv: &Invocation,
    signer: &SelectedSigner,
    prepared: PreparedUpdateMargin,
) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    actions::send_l1_action(
        remote.api(),
        signer,
        prepared.action(),
        actions::nonce_now(),
    )
    .await?;
    let amount = prepared.amount;
    let coin = prepared.coin;

    inv.print(&PositionActionOutput {
        rows: vec![PositionActionConfirmation {
            coin,
            action: "update-margin".to_string(),
            status: "updated".to_string(),
            leverage: None,
            margin_mode: None,
            amount: Some(amount),
        }],
    });
    Ok(())
}

pub fn validate_update_leverage_args(args: &UpdateLeverageArgs) -> Result<(), CliError> {
    if args.leverage == 0 {
        return Err(CliError::Configuration(
            "leverage must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_update_margin_args(args: &UpdateMarginArgs) -> Result<(), CliError> {
    usd_to_micro_units(args.amount, "amount").map(|_| ())
}

fn resolve_perp_asset(resolver: &AssetResolver, coin: &str) -> Result<(String, usize), CliError> {
    let perp = resolver.resolve_perp(coin)?;
    Ok((perp.display_name(), perp.index))
}

fn usd_to_micro_units(value: Decimal, name: &'static str) -> Result<i64, CliError> {
    crate::quantity::scaled_i64(
        value,
        1_000_000,
        CliError::Configuration(format!("{name} must be greater than zero")),
        CliError::Configuration(format!("{name} supports at most 6 decimal places")),
        CliError::Configuration(format!("{name} is too large for Hyperliquid micro-units")),
    )
}

pub const GROUP: GroupSpec =
    GroupSpec::new(&["positions"], "Manage positions").aliases(&["position"]);

#[derive(Args, Debug, Clone)]
pub struct PositionsListArgs {
    #[command(flatten)]
    pub watch: SnapshotWatchArgs,
}

pub struct PositionsList;

impl CliCommand for PositionsList {
    type Args = PositionsListArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["positions", "list"],
        "account",
        "List open positions",
        "List positions for the configured signer.",
    )
    .risk(Risk::AccountState)
    .ows(OwsSupport::ExperimentalFeatureGated)
    .auth(true)
    .stream(StreamBound::SnapshotWatch)
    .args(&[
        arg("watch")
            .kind(InputKind::Boolean)
            .describe("Watch live snapshots. Use max_ticks to bound automation calls."),
        arg("max_ticks").describe("Stop after this many watch snapshots."),
    ])
    .cli_aliases(&["ls"]);

    async fn run(inv: &Invocation, args: &PositionsListArgs) -> Result<(), anyhow::Error> {
        if args.watch.watch {
            watch_positions(inv, args.watch.max_ticks).await
        } else {
            let user = inv.resolve_signer()?.query_address();
            list(inv, user).await
        }
    }
}

pub struct PositionsUpdateLeverage;

impl CliCommand for PositionsUpdateLeverage {
    type Args = UpdateLeverageArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["positions", "update-leverage"],
        "trade",
        "Update leverage for a position",
        "Update leverage for a position.",
    )
    .args(&[
        arg("coin")
            .kind(InputKind::MarketSymbol)
            .describe("Perpetual coin.")
            .resource_id("coin"),
        arg("leverage").describe("Target leverage."),
        arg("isolated")
            .kind(InputKind::Boolean)
            .describe("Use isolated margin mode."),
    ]);

    async fn run(inv: &Invocation, args: &UpdateLeverageArgs) -> Result<(), anyhow::Error> {
        validate_update_leverage_args(args)?;
        let resolver = inv.trading_resolver().await?;
        let prepared = prepare_update_leverage(&resolver, args)?;
        if inv.dry_run() {
            let (signer, acting_as) = dry_run_signing_addresses(inv);
            return print_dry_run(
                inv,
                "positions update-leverage",
                dry_run_signed_details(
                    inv,
                    "update_position_leverage",
                    update_leverage_dry_run_value(
                        inv.remote()?.network().to_string(),
                        args,
                        &prepared,
                    ),
                    signer,
                    acting_as,
                    None,
                ),
            );
        }
        let resolved = inv.resolve_signer()?;
        submit_update_leverage(inv, &resolved.selected_signer(), prepared).await
    }
}

pub struct PositionsUpdateMargin;

impl CliCommand for PositionsUpdateMargin {
    type Args = UpdateMarginArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["positions", "update-margin"],
        "trade",
        "Update margin for a position",
        "Add or remove isolated margin.",
    )
    .args(&[
        arg("coin")
            .kind(InputKind::MarketSymbol)
            .describe("Perpetual coin.")
            .resource_id("coin"),
        arg("amount")
            .kind(InputKind::Amount)
            .describe("Margin amount."),
    ]);

    async fn run(inv: &Invocation, args: &UpdateMarginArgs) -> Result<(), anyhow::Error> {
        validate_update_margin_args(args)?;
        let resolver = inv.trading_resolver().await?;
        let prepared = prepare_update_margin(&resolver, args)?;
        if inv.dry_run() {
            let (signer, acting_as) = dry_run_signing_addresses(inv);
            return print_dry_run(
                inv,
                "positions update-margin",
                dry_run_signed_details(
                    inv,
                    "update_isolated_margin",
                    update_margin_dry_run_value(
                        inv.remote()?.network().to_string(),
                        args,
                        &prepared,
                    ),
                    signer,
                    acting_as,
                    None,
                ),
            );
        }
        let resolved = inv.resolve_signer()?;
        let signer = resolved.selected_signer();
        submit_update_margin(inv, &signer, prepared).await
    }
}

async fn watch_positions(inv: &Invocation, max_ticks: Option<usize>) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let client = remote.api().sdk();
    let resolved = inv.resolve_signer()?;
    let user = resolved.query_address();

    crate::watch::run_snapshot_watch(
        inv.session(),
        "Live positions",
        remote.chain(),
        vec![Subscription::WebData3 { user }],
        max_ticks,
        move || async move { list_snapshot(client, user).await },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_rows_compute_mark_from_value_and_abs_size() {
        let position = serde_json::from_value::<AssetPosition>(serde_json::json!({
            "type": "oneWay",
            "position": {
                "coin": "BTC",
                "szi": "0.1",
                "leverage": {"type": "cross", "value": 5},
                "entryPx": "50000",
                "positionValue": "5100",
                "unrealizedPnl": "100",
                "returnOnEquity": "0.1",
                "liquidationPx": null,
                "marginUsed": "1000",
                "maxLeverage": 50,
                "cumFunding": {"allTime": "0", "sinceOpen": "0", "sinceChange": "0"}
            }
        }))
        .unwrap();

        let row = PositionRow::from(position);
        assert_eq!(row.mark, Decimal::from(51_000));
        assert_eq!(row.side, "long");
    }

    #[test]
    fn update_margin_amount_must_be_positive_and_micro_unit_exact() {
        assert!(usd_to_micro_units(Decimal::new(100_123456, 6), "amount").is_ok());
        assert!(usd_to_micro_units(Decimal::new(1, 7), "amount").is_err());
        assert!(usd_to_micro_units(Decimal::ZERO, "amount").is_err());
    }
}
