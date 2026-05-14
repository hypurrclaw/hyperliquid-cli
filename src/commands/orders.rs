//! Authenticated order management commands.

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, Instant};

use chrono::Utc;
use clap::{Args, ValueEnum};
use either::Either;
use hypersdk::hypercore::types::{
    Action, AssetPosition, BasicOrder, BatchCancel, BatchCancelCloid, BatchModify, BatchOrder,
    Cancel, CancelByCloid, Modify, OrderGrouping, OrderRequest, OrderResponseStatus, OrderType,
    OrderTypePlacement, ScheduleCancel, Side, TimeInForce, TpSl,
};
use hypersdk::hypercore::{Chain, Cloid, HttpClient, OidOrCloid, PriceTick};
use hypersdk::{Address, Decimal};
use serde::{Deserialize, Serialize};

use crate::commands::{
    AssetMetadata, AssetQuery, AssetResolver, PerpAsset, ResolvedAsset, SpotAsset, actions,
    builder, load_raw_spot_markets_from_url, map_api_error, parse_asset_query,
};
use crate::errors::CliError;
use crate::http_api::post_info_json;
use crate::input_hardening::{FilePolicy, read_json_file};
use crate::output::{self, OutputFormat, TableData};
use crate::signing::SelectedSigner;

const DEFAULT_MARKET_ORDER_SLIPPAGE_BPS: u16 = 500;
const MIN_MARKET_ORDER_SLIPPAGE_BPS: u16 = 1;
const MAX_MARKET_ORDER_SLIPPAGE_BPS: u16 = 1_000;
const MAX_BATCH_ORDER_COUNT: usize = 500;
const BPS_DENOMINATOR: i64 = 10_000;

mod args;
mod planning;
mod queries;
mod rendering;
mod validation;

pub use args::*;
use planning::{
    CreateOrderSubmission, prepare_batch_create_order_plan, prepare_cancel_all_orders_plan,
    prepare_cancel_order_plan, prepare_create_order_plan, prepare_modify_order_plan,
    prepare_position_tpsl_batch, prepare_scale_batch, prepare_schedule_cancel_plan,
    prepare_twap_cancel_plan, prepare_twap_create_plan,
};
pub use planning::{
    OrderDryRunPlan, batch_create_dry_run_args, batch_create_dry_run_plan, cancel_all_dry_run_plan,
    cancel_dry_run_plan, create_dry_run_args, create_dry_run_plan, create_dry_run_preview,
    modify_dry_run_plan, scale_dry_run_args, scale_dry_run_plan, schedule_cancel_dry_run_plan,
    tpsl_dry_run_plan, tpsl_dry_run_preview, twap_cancel_dry_run_plan, twap_create_dry_run_plan,
};
#[cfg(test)]
use planning::{build_position_tpsl_batch, prepare_normal_tpsl_batch, prepare_order};
pub use queries::{history, open, open_snapshot, status, status_query};
pub use rendering::OrderListOutput;
use rendering::{
    CancelAllSummary, CancelAllSummaryOutput, CancelConfirmation, CancelConfirmationOutput,
    ModifyConfirmation, ModifyConfirmationOutput, OrderConfirmation, OrderConfirmationOutput,
    OrderListRow, ScheduleCancelConfirmation, ScheduleCancelOutput, TpslOrderConfirmation,
    TpslOrderConfirmationOutput, TpslResponseStatus, TwapCancelConfirmation, TwapCancelOutput,
    TwapCreateConfirmation, TwapCreateOutput,
};
pub use validation::{
    read_validated_batch_create_orders, validate_batch_create_args, validate_create_args,
    validate_create_resolved_asset, validate_modify_args, validate_scale_args, validate_tpsl_args,
    validate_twap_create_args,
};

#[derive(Debug, Clone)]
struct PreparedOrder {
    request: OrderRequest,
    asset_kind: TradableAssetKind,
    coin: String,
    side: OrderSide,
    order_type: CreateOrderType,
    tif: Option<TifArg>,
    price: Decimal,
    trigger_price: Option<Decimal>,
    size: Decimal,
    amount: Option<Decimal>,
    amount_unit: String,
    warning: Option<String>,
    builder: Option<OrderBuilderFee>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct OrderBuilderFee {
    b: String,
    f: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TradableAssetKind {
    Perp,
    Spot,
    Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TpslLegKind {
    Parent,
    TakeProfit,
    StopLoss,
}

impl fmt::Display for TpslLegKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parent => write!(f, "parent"),
            Self::TakeProfit => write!(f, "take_profit"),
            Self::StopLoss => write!(f, "stop_loss"),
        }
    }
}

#[derive(Debug, Clone)]
struct PreparedTpslLeg {
    request: OrderRequest,
    leg: TpslLegKind,
    coin: String,
    side: OrderSide,
    order_type: CreateOrderType,
    price: Decimal,
    size: Decimal,
    tif: Option<TifArg>,
    reduce_only: bool,
    warning: Option<String>,
}

#[derive(Debug, Clone)]
struct PreparedTpslBatch {
    batch: BatchOrder,
    grouping: TpslGroupingArg,
    legs: Vec<PreparedTpslLeg>,
}

#[derive(Debug, Clone)]
struct PreparedOrderLeg {
    leg_index: usize,
    order: PreparedOrder,
}

#[derive(Debug, Clone)]
struct PreparedOrderBatch {
    batch: BatchOrder,
    legs: Vec<PreparedOrderLeg>,
}

#[derive(Debug, Clone)]
enum TradableAsset {
    Perp {
        name: String,
        index: usize,
        dex: Option<String>,
        sz_decimals: u32,
        collateral: String,
    },
    Spot {
        symbol: String,
        index: usize,
        base_sz_decimals: u32,
        quote: String,
    },
    Outcome {
        notation: String,
        asset_id: usize,
    },
}

#[derive(Debug, Clone)]
struct MidLookup {
    coin: String,
    dex: Option<String>,
    original_coin: String,
}

#[derive(Debug, Clone)]
enum OrderIdentifier {
    Oid(u64),
    Cloid { raw: String, parsed: Cloid },
}

impl OrderIdentifier {
    fn to_status_identifier(&self) -> OidOrCloid {
        match self {
            Self::Oid(oid) => Either::Left(*oid),
            Self::Cloid { parsed, .. } => Either::Right(*parsed),
        }
    }

    fn display(&self) -> String {
        match self {
            Self::Oid(oid) => oid.to_string(),
            Self::Cloid { raw, .. } => raw.clone(),
        }
    }
}

pub struct OrderExecutionContext<'a> {
    pub submission: OrderSubmissionContext<'a>,
    pub client: &'a HttpClient,
    pub resolver: &'a AssetResolver,
}

#[derive(Clone, Copy)]
pub struct OrderSubmissionContext<'a> {
    pub api_base_url: &'a str,
    pub chain: Chain,
    pub signer: &'a SelectedSigner,
    pub require_mainnet_confirmation: bool,
}

/// Place an order through hypersdk's authenticated `/exchange` flow.
pub async fn create(
    context: OrderExecutionContext<'_>,
    args: &CreateArgs,
    vault_address: Option<Address>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let plan = prepare_create_order_plan(context.client, context.resolver, args).await?;
    match plan.submission {
        CreateOrderSubmission::NormalTpsl(prepared) => {
            if context.submission.require_mainnet_confirmation
                && !args.yes
                && !confirm_mainnet_tpsl_batch(&prepared, format)?
            {
                return Err(CliError::Configuration(
                    "Mainnet TP/SL confirmation required; order placement cancelled. Rerun with --yes for deliberate automation."
                        .to_string(),
                )
                .into());
            }

            let statuses = place_tpsl_batch(
                context.submission.api_base_url,
                context.submission.chain,
                context.submission.signer,
                prepared.batch.clone(),
                vault_address,
            )
            .await?;
            let rows = tpsl_confirmation_rows(&prepared, statuses)?;
            output::print_data(
                &TpslOrderConfirmationOutput { rows },
                format,
                start.elapsed(),
            );
            Ok(())
        }
        CreateOrderSubmission::Single(prepared) => {
            if context.submission.require_mainnet_confirmation
                && !args.yes
                && !confirm_mainnet_order(&prepared, format)?
            {
                return Err(CliError::Configuration(
                    "Mainnet order confirmation required; order placement cancelled. Rerun with --yes for deliberate automation."
                        .to_string(),
                )
                .into());
            }

            let batch = BatchOrder {
                orders: vec![prepared.request.clone()],
                grouping: OrderGrouping::Na,
            };
            let statuses = if let Some(builder) = prepared.builder.clone() {
                place_builder_order_batch_raw(
                    context.submission.api_base_url,
                    context.submission.chain,
                    context.submission.signer,
                    batch,
                    builder,
                    vault_address,
                )
                .await?
            } else {
                place_order_batch_raw(
                    context.submission.api_base_url,
                    context.submission.chain,
                    context.submission.signer,
                    batch,
                    vault_address,
                )
                .await?
            };

            let rows = statuses
                .into_iter()
                .map(|status| OrderConfirmation::from_status(&prepared, status))
                .collect::<Result<Vec<_>, _>>()?;
            output::print_data(&OrderConfirmationOutput { rows }, format, start.elapsed());
            Ok(())
        }
    }
}

/// Place a generated scale order batch.
pub async fn scale(
    context: OrderExecutionContext<'_>,
    args: &ScaleArgs,
    vault_address: Option<Address>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let prepared = prepare_scale_batch(context.client, context.resolver, args).await?;
    if context.submission.require_mainnet_confirmation
        && !args.yes
        && !confirm_mainnet_order_batch("scale", &prepared, format)?
    {
        return Err(CliError::Configuration(
            "Mainnet scale order confirmation required; order placement cancelled. Rerun with --yes for deliberate automation."
                .to_string(),
        )
        .into());
    }

    let rows = place_order_batch(context, prepared, vault_address).await?;
    output::print_data(&OrderConfirmationOutput { rows }, format, start.elapsed());
    Ok(())
}

/// Place an explicit JSON order batch.
pub async fn batch_create(
    context: OrderExecutionContext<'_>,
    args: &BatchCreateArgs,
    orders: Vec<BatchCreateOrder>,
    vault_address: Option<Address>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let plan =
        prepare_batch_create_order_plan(context.client, context.resolver, args, orders).await?;
    let prepared = plan.prepared;
    if context.submission.require_mainnet_confirmation
        && !args.yes
        && !confirm_mainnet_order_batch("batch-create", &prepared, format)?
    {
        return Err(CliError::Configuration(
            "Mainnet batch order confirmation required; order placement cancelled. Rerun with --yes for deliberate automation."
                .to_string(),
        )
        .into());
    }

    let rows = place_order_batch(context, prepared, vault_address).await?;
    output::print_data(&OrderConfirmationOutput { rows }, format, start.elapsed());
    Ok(())
}

async fn place_order_batch(
    context: OrderExecutionContext<'_>,
    prepared: PreparedOrderBatch,
    vault_address: Option<Address>,
) -> Result<Vec<OrderConfirmation>, CliError> {
    let statuses = place_order_batch_raw(
        context.submission.api_base_url,
        context.submission.chain,
        context.submission.signer,
        prepared.batch.clone(),
        vault_address,
    )
    .await?;

    if statuses.len() != prepared.legs.len() {
        return Err(CliError::Internal(anyhow::anyhow!(
            "exchange returned {} statuses for {} order legs",
            statuses.len(),
            prepared.legs.len()
        )));
    }

    prepared
        .legs
        .iter()
        .zip(statuses)
        .map(|(leg, status)| OrderConfirmation::from_status(&leg.order, status))
        .collect()
}

/// Place position-attached take-profit and/or stop-loss trigger orders.
pub async fn tpsl(
    context: OrderExecutionContext<'_>,
    args: &TpslArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let prepared = prepare_position_tpsl_batch(
        context.client,
        context.resolver,
        context.submission.signer,
        args,
    )
    .await?;
    if context.submission.require_mainnet_confirmation
        && !args.yes
        && !confirm_mainnet_tpsl_batch(&prepared, format)?
    {
        return Err(CliError::Configuration(
            "Mainnet TP/SL confirmation required; order placement cancelled. Rerun with --yes for deliberate automation."
                .to_string(),
        )
        .into());
    }

    let statuses = place_tpsl_batch(
        context.submission.api_base_url,
        context.submission.chain,
        context.submission.signer,
        prepared.batch.clone(),
        None,
    )
    .await?;
    let rows = tpsl_confirmation_rows(&prepared, statuses)?;
    output::print_data(
        &TpslOrderConfirmationOutput { rows },
        format,
        start.elapsed(),
    );
    Ok(())
}

/// Cancel a single open order by exchange OID or client order ID.
pub async fn cancel(
    context: OrderExecutionContext<'_>,
    user: Address,
    args: &CancelArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    context
        .submission
        .signer
        .ensure_can_attempt_live_signing()?;
    let start = Instant::now();
    let plan = prepare_cancel_order_plan(context.client, context.resolver, user, args).await?;
    let nonce = Utc::now().timestamp_millis() as u64;
    let response = actions::send_l1_action_raw(
        context.submission.api_base_url,
        context.submission.chain,
        context.submission.signer,
        plan.action,
        nonce,
        None,
        "cancel failed",
    )
    .await?;
    let statuses = order_statuses_from_response(response, "cancel")?;

    ensure_action_statuses_ok(&statuses, "cancel")?;
    output::print_data(
        &CancelConfirmationOutput {
            rows: vec![CancelConfirmation {
                coin: plan.coin,
                status: "cancelled".to_string(),
                order_id: plan.order_id,
                cloid: plan.cloid,
            }],
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

/// Cancel all open orders, optionally filtered by coin.
pub async fn cancel_all(
    context: OrderExecutionContext<'_>,
    user: Address,
    args: &CancelAllArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    context
        .submission
        .signer
        .ensure_can_attempt_live_signing()?;
    let start = Instant::now();
    let plan = prepare_cancel_all_orders_plan(context.client, context.resolver, user, args).await?;

    if plan.cancelled_orders != 0 && !args.yes && !confirm_cancel_all(args.coin.as_deref(), format)?
    {
        return Err(CliError::Configuration("cancel-all aborted".to_string()).into());
    }

    let cancelled_orders = if let Some(action) = plan.action {
        let nonce = Utc::now().timestamp_millis() as u64;
        let response = actions::send_l1_action_raw(
            context.submission.api_base_url,
            context.submission.chain,
            context.submission.signer,
            action,
            nonce,
            None,
            "cancel-all failed",
        )
        .await?;
        let statuses = order_statuses_from_response(response, "cancel-all")?;
        ensure_action_statuses_ok(&statuses, "cancel-all")?
    } else {
        0
    };

    output::print_data(
        &CancelAllSummaryOutput {
            rows: vec![CancelAllSummary {
                coin: plan.summary_coin,
                cancelled_orders,
            }],
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

/// Modify an existing order's price and/or size.
pub async fn modify(
    context: OrderExecutionContext<'_>,
    user: Address,
    args: &ModifyArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    context
        .submission
        .signer
        .ensure_can_attempt_live_signing()?;
    let start = Instant::now();
    let plan = prepare_modify_order_plan(context.client, context.resolver, user, args).await?;
    let nonce = Utc::now().timestamp_millis() as u64;
    let response = actions::send_l1_action_raw(
        context.submission.api_base_url,
        context.submission.chain,
        context.submission.signer,
        plan.action,
        nonce,
        None,
        "modify failed",
    )
    .await?;
    let statuses = order_statuses_from_response(response, "modify")?;
    ensure_action_statuses_ok(&statuses, "modify")?;
    let mut confirmation = plan.confirmation;
    confirmation.order_id = modification_result_oid(&statuses).or(confirmation.order_id);

    output::print_data(
        &ModifyConfirmationOutput {
            rows: vec![confirmation],
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

/// Create a TWAP order using Hyperliquid's signed TWAP exchange action.
pub async fn twap_create(
    context: OrderSubmissionContext<'_>,
    resolver: &AssetResolver,
    args: &TwapCreateArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    context.signer.ensure_can_attempt_live_signing()?;
    let plan = prepare_twap_create_plan(resolver, args)?;
    if context.require_mainnet_confirmation
        && !args.yes
        && !confirm_mainnet_twap(&plan.coin, args, format)?
    {
        return Err(CliError::Configuration(
            "Mainnet TWAP confirmation required; TWAP creation cancelled. Rerun with --yes for deliberate automation."
                .to_string(),
        )
        .into());
    }
    let response = actions::send_raw_l1_json_action(
        context.api_base_url,
        context.chain,
        context.signer,
        &plan.action,
        actions::RawL1ActionMetadata::new(actions::nonce_now()),
        "twap-create rejected",
    )
    .await?;
    let twap_id = parse_twap_create_response(response)?;

    output::print_data(
        &TwapCreateOutput {
            rows: vec![TwapCreateConfirmation {
                coin: plan.coin,
                side: plan.side,
                size: plan.size,
                duration_seconds: plan.duration_seconds,
                duration_minutes: plan.duration_minutes,
                status: "running".to_string(),
                twap_id,
            }],
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

/// Cancel a TWAP order using Hyperliquid's signed TWAP cancel action.
pub async fn twap_cancel(
    context: OrderSubmissionContext<'_>,
    resolver: &AssetResolver,
    args: &TwapCancelArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    context.signer.ensure_can_attempt_live_signing()?;
    let plan = prepare_twap_cancel_plan(resolver, args)?;
    let response = actions::send_raw_l1_json_action(
        context.api_base_url,
        context.chain,
        context.signer,
        &plan.action,
        actions::RawL1ActionMetadata::new(actions::nonce_now()),
        "twap-cancel rejected",
    )
    .await?;
    parse_twap_cancel_response(response)?;

    output::print_data(
        &TwapCancelOutput {
            rows: vec![TwapCancelConfirmation {
                coin: plan.coin,
                status: "cancelled".to_string(),
                twap_id: plan.twap_id,
            }],
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

/// Schedule a dead man's switch cancel-all.
pub async fn schedule_cancel(
    context: OrderSubmissionContext<'_>,
    args: &ScheduleCancelArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    context.signer.ensure_can_attempt_live_signing()?;
    let plan = prepare_schedule_cancel_plan(args, Utc::now())?;
    let nonce = actions::nonce_now();
    if plan.scheduled_at.is_some() {
        actions::send_l1_action(
            context.api_base_url,
            context.chain,
            context.signer,
            plan.action,
            nonce,
        )
        .await?;
    } else {
        actions::send_raw_l1_json_action(
            context.api_base_url,
            context.chain,
            context.signer,
            &ScheduleCancelRawAction {
                action_type: "scheduleCancel",
                time: None,
            },
            actions::RawL1ActionMetadata::new(nonce),
            "schedule-cancel clear rejected",
        )
        .await?;
    }

    output::print_data(
        &ScheduleCancelOutput {
            rows: vec![ScheduleCancelConfirmation {
                status: if plan.scheduled_at.is_some() {
                    "scheduled".to_string()
                } else {
                    "cleared".to_string()
                },
                scheduled_time: plan.scheduled_at.as_ref().map(|value| value.to_rfc3339()),
                scheduled_time_ms: plan
                    .scheduled_at
                    .as_ref()
                    .map(|value| value.timestamp_millis() as u64),
                in_seconds: plan.in_seconds,
            }],
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

#[derive(serde::Deserialize)]
struct SpotMetaTokenName {
    name: String,
    index: usize,
}

fn collateral_name_from_tokens(
    token_names: &[SpotMetaTokenName],
    collateral_token: Option<usize>,
) -> String {
    collateral_token
        .and_then(|token| {
            token_names
                .iter()
                .find(|candidate| candidate.index == token)
                .or_else(|| token_names.get(token))
        })
        .map(|token| token.name.clone())
        .unwrap_or_else(|| "USDC".to_string())
}

/// Load trading metadata directly from Hyperliquid's raw info endpoints.
///
/// This is used as a trading-command fallback when the full SDK market loader cannot decode
/// ancillary metadata on a network. Order placement itself still goes through
/// [`HttpClient::place`] for SDK signing and submission.
pub async fn load_perp_resolver_from_api_base(
    api_base_url: &str,
) -> Result<AssetResolver, CliError> {
    #[derive(serde::Deserialize)]
    struct MetaResponse {
        universe: Vec<MetaPerp>,
        #[serde(rename = "collateralToken")]
        collateral_token: Option<usize>,
    }

    #[derive(serde::Deserialize)]
    struct MetaPerp {
        name: String,
        #[serde(rename = "szDecimals")]
        sz_decimals: u32,
    }

    #[derive(serde::Deserialize)]
    struct RawPerpDex {
        name: String,
    }

    #[derive(serde::Deserialize)]
    struct SpotMetaResponse {
        tokens: Vec<SpotMetaTokenName>,
    }

    let token_names = post_info_json::<SpotMetaResponse>(
        api_base_url,
        &serde_json::json!({ "type": "spotMeta" }),
        "loading spot token metadata",
    )
    .await
    .map(|spot_meta| spot_meta.tokens)
    .unwrap_or_default();
    let collateral_name =
        |collateral_token| collateral_name_from_tokens(&token_names, collateral_token);

    let meta = post_info_json::<MetaResponse>(
        api_base_url,
        &serde_json::json!({ "type": "meta" }),
        "loading perpetual metadata",
    )
    .await?;

    let default_collateral = collateral_name(meta.collateral_token);

    let mut perps = meta
        .universe
        .into_iter()
        .enumerate()
        .map(|(index, perp)| PerpAsset {
            name: perp.name,
            index,
            dex: None,
            sz_decimals: perp.sz_decimals,
            collateral: default_collateral.clone(),
        })
        .collect::<Vec<_>>();

    // Fallback metadata must mirror hypersdk's HIP-3 asset ID formula:
    // 100_000 + dex_slot * 10_000 + market_index.
    // See Hyperliquid docs: for-developers/api/asset-ids.
    if let Ok(dexes) = post_info_json::<Vec<Option<RawPerpDex>>>(
        api_base_url,
        &serde_json::json!({ "type": "perpDexs" }),
        "loading HIP-3 DEX metadata",
    )
    .await
    {
        for (dex_index, dex) in dexes.into_iter().enumerate() {
            let Some(dex) = dex else { continue };
            if let Ok(meta) = post_info_json::<MetaResponse>(
                api_base_url,
                &serde_json::json!({ "type": "meta", "dex": dex.name.clone() }),
                "loading HIP-3 perpetual metadata",
            )
            .await
            {
                let prefix = format!("{}:", dex.name);
                let collateral = collateral_name(meta.collateral_token);
                perps.extend(meta.universe.into_iter().enumerate().map(|(index, perp)| {
                    let name = perp
                        .name
                        .strip_prefix(&prefix)
                        .unwrap_or(perp.name.as_str())
                        .to_string();
                    PerpAsset {
                        name,
                        index: 100_000 + dex_index * 10_000 + index,
                        dex: Some(dex.name.clone()),
                        sz_decimals: perp.sz_decimals,
                        collateral: collateral.clone(),
                    }
                }));
            }
        }
    }

    let spots = load_raw_spot_markets_from_url(
        reqwest::Url::parse(api_base_url)
            .map_err(|err| CliError::Configuration(format!("invalid API base URL: {err}")))?,
    )
    .await?
    .into_iter()
    .map(|market| SpotAsset {
        symbol: market.symbol,
        index: market.index,
        base: market.base,
        quote: market.quote,
        base_sz_decimals: u32::try_from(market.base_sz_decimals).unwrap_or_default(),
    })
    .collect();

    Ok(AssetResolver::new(AssetMetadata::from_assets(perps, spots)))
}

fn order_builder_fee(
    args: &CreateArgs,
    asset_kind: TradableAssetKind,
) -> Result<Option<OrderBuilderFee>, CliError> {
    let address: Address;
    let fee: u64;
    match (args.builder.as_deref(), args.builder_fee_rate.as_deref()) {
        (Some(raw_builder), Some(raw_fee)) => {
            address = builder::parse_builder_address(raw_builder)?;
            fee = builder::validate_max_fee_rate(raw_fee)?;
        }
        (Some(_), None) => {
            return Err(CliError::Configuration(
                "orders create --builder requires --builder-fee-rate".to_string(),
            ));
        }
        (None, Some(_)) => {
            return Err(CliError::Configuration(
                "orders create --builder-fee-rate requires --builder".to_string(),
            ));
        }
        (None, None) => match builder::resolve_default_builder_fee()? {
            Some((default_addr, default_fee)) => {
                address = default_addr;
                fee = default_fee;
            }
            None => return Ok(None),
        },
    };
    if asset_kind == TradableAssetKind::Perp && fee > 100 {
        return Err(CliError::Unsupported(
            "perp order builder fee rate cannot exceed 0.1%".to_string(),
        ));
    }
    Ok(Some(OrderBuilderFee {
        // Hyperliquid's Python SDK lowercases builder addresses before hashing/signing
        // L1 order actions. Checksum casing can change the msgpack action hash and
        // recover a random nonexistent signer on exchange validation.
        b: address.to_string().to_ascii_lowercase(),
        f: fee,
    }))
}

fn read_batch_create_orders(path: &Path) -> Result<Vec<BatchCreateOrder>, CliError> {
    let value = read_json_file(path, FilePolicy::json_artifact("orders file"))?;
    let orders_value = value.get("orders").cloned().unwrap_or(value);
    let orders = serde_json::from_value::<Vec<BatchCreateOrder>>(orders_value).map_err(|err| {
        CliError::Configuration(format!("invalid orders file {}: {err}", path.display()))
    })?;
    if orders.len() > MAX_BATCH_ORDER_COUNT {
        return Err(CliError::Configuration(format!(
            "orders batch-create supports at most {MAX_BATCH_ORDER_COUNT} order legs"
        )));
    }
    Ok(orders)
}

fn scale_prices(
    start_price: Decimal,
    end_price: Decimal,
    order_count: usize,
) -> Result<Vec<Decimal>, CliError> {
    if order_count == 0 {
        return Err(CliError::Configuration(
            "orders scale requires --orders greater than zero".to_string(),
        ));
    }
    if order_count == 1 {
        return Ok(vec![start_price]);
    }
    let steps = Decimal::from_str(&(order_count - 1).to_string())
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    let step = (end_price - start_price) / steps;
    Ok((0..order_count)
        .map(|index| {
            let index_decimal =
                Decimal::from_str(&index.to_string()).expect("usize index should parse as Decimal");
            start_price + step * index_decimal
        })
        .collect())
}

fn scale_sizes(total_size: Decimal, order_count: usize) -> Result<Vec<Decimal>, CliError> {
    if order_count == 0 {
        return Err(CliError::Configuration(
            "orders scale requires --orders greater than zero".to_string(),
        ));
    }
    let count = Decimal::from_str(&order_count.to_string())
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    let per_leg = total_size / count;
    if per_leg <= Decimal::ZERO {
        return Err(CliError::Configuration(
            "orders scale total size is too small to distribute across requested orders"
                .to_string(),
        ));
    }
    if per_leg * count != total_size {
        return Err(CliError::Configuration(
            "orders scale total size cannot be evenly distributed across requested orders"
                .to_string(),
        ));
    }
    Ok(vec![per_leg; order_count])
}

async fn lookup_position_for_tpsl(
    client: &HttpClient,
    user: Address,
    dex: Option<String>,
    coin: &str,
) -> Result<AssetPosition, CliError> {
    client
        .clearinghouse_state(user, dex)
        .await
        .map_err(map_api_error)?
        .asset_positions
        .into_iter()
        .find(|position| position.position.coin.eq_ignore_ascii_case(coin))
        .ok_or_else(|| {
            CliError::Configuration(format!(
                "orders tpsl requires an open {coin} position or both --side and --size for a fixed-size TP/SL"
            ))
        })
}

fn position_close_side_and_size(
    position: &AssetPosition,
) -> Result<(OrderSide, Decimal), CliError> {
    let szi = position.position.szi;
    if szi > Decimal::ZERO {
        Ok((OrderSide::Sell, szi))
    } else if szi < Decimal::ZERO {
        Ok((OrderSide::Buy, szi.abs()))
    } else {
        Err(CliError::Configuration(format!(
            "orders tpsl requires a non-zero {} position",
            position.position.coin
        )))
    }
}

fn tpsl_confirmation_rows(
    prepared: &PreparedTpslBatch,
    statuses: Vec<TpslResponseStatus>,
) -> Result<Vec<TpslOrderConfirmation>, CliError> {
    if statuses.len() != prepared.legs.len() {
        return Err(CliError::Internal(anyhow::anyhow!(
            "order placement returned {} statuses for {} TP/SL legs",
            statuses.len(),
            prepared.legs.len()
        )));
    }
    prepared
        .legs
        .iter()
        .zip(statuses)
        .map(|(leg, status)| {
            TpslOrderConfirmation::from_tpsl_status(leg, prepared.grouping, status)
        })
        .collect()
}

async fn place_tpsl_batch(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    batch: BatchOrder,
    vault_address: Option<Address>,
) -> Result<Vec<TpslResponseStatus>, CliError> {
    let response = actions::send_l1_action_raw(
        api_base_url,
        chain,
        signer,
        Action::Order(batch),
        actions::nonce_now(),
        vault_address,
        "order placement failed",
    )
    .await?;
    parse_tpsl_order_statuses(response)
}

async fn place_order_batch_raw(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    batch: BatchOrder,
    vault_address: Option<Address>,
) -> Result<Vec<OrderResponseStatus>, CliError> {
    let response = actions::send_l1_action_raw(
        api_base_url,
        chain,
        signer,
        Action::Order(batch),
        actions::nonce_now(),
        vault_address,
        "order placement failed",
    )
    .await?;
    parse_order_statuses(response)
}

#[derive(Debug, Serialize)]
struct BuilderOrderAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    orders: Vec<OrderRequest>,
    grouping: OrderGrouping,
    builder: OrderBuilderFee,
}

async fn place_builder_order_batch_raw(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    batch: BatchOrder,
    builder: OrderBuilderFee,
    vault_address: Option<Address>,
) -> Result<Vec<OrderResponseStatus>, CliError> {
    // Builder fee order shape is documented by Hyperliquid's exchange endpoint:
    // action `{ type: "order", orders, grouping, builder: { b, f } }`, where
    // `f` is measured in tenths of a basis point.
    let action = BuilderOrderAction {
        action_type: "order",
        orders: batch.orders,
        grouping: batch.grouping,
        builder,
    };
    let response = actions::send_raw_l1_json_action(
        api_base_url,
        chain,
        signer,
        &action,
        actions::RawL1ActionMetadata::new(actions::nonce_now()).with_vault_address(vault_address),
        "order placement failed",
    )
    .await?;
    parse_order_statuses(response)
}

fn parse_order_statuses(response: serde_json::Value) -> Result<Vec<OrderResponseStatus>, CliError> {
    if response.get("type").and_then(serde_json::Value::as_str) != Some("order") {
        return Err(CliError::Internal(anyhow::anyhow!(
            "order placement response had unexpected type: {response}"
        )));
    }
    let statuses = response
        .get("data")
        .and_then(|data| data.get("statuses"))
        .cloned()
        .ok_or_else(|| {
            CliError::Internal(anyhow::anyhow!(
                "order placement response missing statuses: {response}"
            ))
        })?;
    serde_json::from_value(statuses).map_err(|err| {
        CliError::Internal(anyhow::anyhow!(
            "order placement response decode failed: {err}; body={response}"
        ))
    })
}

fn parse_tpsl_order_statuses(
    response: serde_json::Value,
) -> Result<Vec<TpslResponseStatus>, CliError> {
    if response.get("type").and_then(serde_json::Value::as_str) != Some("order") {
        return Err(CliError::Internal(anyhow::anyhow!(
            "order placement response had unexpected type: {response}"
        )));
    }
    let statuses = response
        .get("data")
        .and_then(|data| data.get("statuses"))
        .cloned()
        .ok_or_else(|| {
            CliError::Internal(anyhow::anyhow!(
                "order placement response missing statuses: {response}"
            ))
        })?;
    serde_json::from_value(statuses).map_err(|err| {
        CliError::Internal(anyhow::anyhow!(
            "order placement response decode failed: {err}; body={response}"
        ))
    })
}

async fn resolve_tradable_asset(
    client: &HttpClient,
    resolver: &AssetResolver,
    query: &str,
    args: &CreateArgs,
) -> Result<TradableAsset, CliError> {
    if let AssetQuery::Outcome(trimmed_notation) = parse_asset_query(&args.coin) {
        if args.dex.is_some() {
            return Err(CliError::Unsupported(format!(
                "outcome notation '{trimmed_notation}' cannot be used with --dex; omit --dex for outcome orders"
            )));
        }
        if args.order_type == CreateOrderType::Market {
            return Err(CliError::Unsupported(
                "outcome orders do not support --type market because outcome size decimals are not verified; use an explicit --price and --size"
                    .to_string(),
            ));
        }
        let notation = crate::commands::outcomes::parse_outcome_notation(&trimmed_notation)?;
        let asset_id = crate::commands::outcomes::outcome_asset_id(notation.encoding)?;
        let asset_id = usize::try_from(asset_id).map_err(|_| {
            CliError::Unsupported("outcome asset id is outside supported bounds".to_string())
        })?;
        return Ok(TradableAsset::Outcome {
            notation: trimmed_notation,
            asset_id,
        });
    }
    match resolver.resolve_perp(query) {
        Ok(ResolvedAsset::Perp {
            name,
            index,
            dex,
            sz_decimals,
            collateral,
        }) => Ok(TradableAsset::Perp {
            name,
            index,
            dex,
            sz_decimals,
            collateral,
        }),
        Ok(ResolvedAsset::Spot { .. }) => unreachable!("resolve_perp never returns spot assets"),
        Err(perp_err) => {
            if let AssetQuery::Hip3 { dex, token } = parse_asset_query(query) {
                return match resolve_hip3_tradable_asset(client, query, &dex, &token).await {
                    Ok(asset) => Ok(asset),
                    Err(CliError::AssetNotFoundNoSuggestion { .. }) => Err(perp_err),
                    Err(err) => Err(err),
                };
            }
            if args.dex.is_some() {
                return Err(perp_err);
            }
            match resolver.resolve_spot(&args.coin) {
                Ok(ResolvedAsset::Spot {
                    symbol,
                    index,
                    base_sz_decimals,
                    quote,
                    ..
                }) => Ok(TradableAsset::Spot {
                    symbol,
                    index,
                    base_sz_decimals,
                    quote,
                }),
                Ok(ResolvedAsset::Perp { .. }) => unreachable!("resolve_spot never returns perps"),
                Err(_) => Err(perp_err),
            }
        }
    }
}

async fn resolve_hip3_tradable_asset(
    client: &HttpClient,
    input: &str,
    dex_name: &str,
    token: &str,
) -> Result<TradableAsset, CliError> {
    let dex = client
        .perp_dexs()
        .await
        .map_err(map_api_error)?
        .into_iter()
        .find(|dex| dex.name().eq_ignore_ascii_case(dex_name))
        .ok_or_else(|| CliError::Unsupported(format!("Unknown DEX: {dex_name}")))?;
    let canonical_dex_name = dex.name().to_string();
    let prefix = format!("{canonical_dex_name}:");
    let market = client
        .perps_from(dex)
        .await
        .map_err(map_api_error)?
        .into_iter()
        .find(|market| {
            let display_token = market.name.strip_prefix(&prefix).unwrap_or(&market.name);
            display_token.eq_ignore_ascii_case(token) || market.name.eq_ignore_ascii_case(token)
        })
        .ok_or_else(|| CliError::AssetNotFoundNoSuggestion {
            asset: input.to_string(),
        })?;
    let name = market
        .name
        .strip_prefix(&prefix)
        .unwrap_or(market.name.as_str())
        .to_string();

    Ok(TradableAsset::Perp {
        name,
        index: market.index,
        dex: Some(canonical_dex_name),
        sz_decimals: u32::try_from(market.sz_decimals).unwrap_or_default(),
        collateral: market.collateral.name,
    })
}

fn spot_mid_key(asset_index: usize) -> String {
    format!("@{}", asset_index.saturating_sub(10_000))
}

fn market_trigger_price(args: &CreateArgs) -> Result<Decimal, CliError> {
    args.trigger_price.or(args.price).ok_or_else(|| {
        CliError::Configuration(
            "orders create requires --trigger-price or --price for market trigger orders"
                .to_string(),
        )
    })
}

fn duration_seconds_to_minutes(seconds: u64) -> u64 {
    seconds.div_ceil(60)
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum TwapExchangeAction {
    TwapOrder { twap: TwapOrderAction },
    TwapCancel { a: usize, t: u64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleCancelRawAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    time: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct TwapOrderAction {
    a: usize,
    b: bool,
    s: String,
    r: bool,
    m: u64,
    t: bool,
}

fn parse_twap_create_response(response: serde_json::Value) -> Result<u64, CliError> {
    if let Some(error) = response
        .pointer("/data/status/error")
        .and_then(serde_json::Value::as_str)
    {
        return Err(CliError::Unsupported(format!(
            "twap-create rejected: {error}"
        )));
    }

    response
        .pointer("/data/status/running/twapId")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            CliError::Internal(anyhow::anyhow!(
                "twap-create returned unexpected response: {}",
                response
            ))
        })
}

fn parse_twap_cancel_response(response: serde_json::Value) -> Result<(), CliError> {
    let status = response.pointer("/data/status").ok_or_else(|| {
        CliError::Internal(anyhow::anyhow!(
            "twap-cancel returned unexpected response: {}",
            response
        ))
    })?;

    if status.as_str() == Some("success") {
        return Ok(());
    }

    if let Some(error) = status.get("error").and_then(serde_json::Value::as_str) {
        return Err(CliError::Unsupported(format!(
            "twap-cancel rejected: {error}"
        )));
    }

    Err(CliError::Internal(anyhow::anyhow!(
        "twap-cancel returned unexpected status: {status}"
    )))
}

fn parse_cancel_identifier(args: &CancelArgs) -> Result<OrderIdentifier, CliError> {
    if let Some(order_id) = args.order_id {
        return Ok(OrderIdentifier::Oid(order_id));
    }

    let cloid = args.cloid.as_ref().ok_or_else(|| {
        CliError::Configuration("orders cancel requires ORDER_ID or --cloid".to_string())
    })?;
    Ok(OrderIdentifier::Cloid {
        raw: cloid.clone(),
        parsed: parse_lookup_cloid(cloid)?,
    })
}

fn parse_modify_identifier(args: &ModifyArgs) -> Result<OrderIdentifier, CliError> {
    if let Some(order_id) = args.order_id {
        return Ok(OrderIdentifier::Oid(order_id));
    }

    let cloid = args.cloid.as_ref().ok_or_else(|| {
        CliError::Configuration("orders modify requires ORDER_ID or --cloid".to_string())
    })?;
    Ok(OrderIdentifier::Cloid {
        raw: cloid.clone(),
        parsed: parse_cloid(cloid)?,
    })
}

fn parse_cloid(input: &str) -> Result<Cloid, CliError> {
    let Some(stripped) = input.strip_prefix("0x") else {
        return Err(CliError::Configuration(
            "CLOID must be a 0x-prefixed 16-byte hex value".to_string(),
        ));
    };

    if stripped.len() != 32 {
        return Err(CliError::Configuration(format!(
            "CLOID must be exactly 16 bytes (32 hex characters after 0x); got {} hex characters",
            stripped.len()
        )));
    }

    if !stripped.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(CliError::Configuration(
            "CLOID must contain only hexadecimal characters after 0x".to_string(),
        ));
    }

    input.parse::<Cloid>().map_err(|err| {
        CliError::Configuration(format!("CLOID must be a valid 16-byte hex value: {err}"))
    })
}

fn parse_lookup_cloid(input: &str) -> Result<Cloid, CliError> {
    let stripped = input.strip_prefix("0x").unwrap_or(input);
    if stripped.is_empty()
        || stripped.len() > 32
        || !stripped.chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return Err(CliError::Configuration(
            "CLOID must be a 0x-prefixed hex value up to 16 bytes".to_string(),
        ));
    }

    let padded = format!("0x{stripped:0>32}");
    padded.parse::<Cloid>().map_err(|err| {
        CliError::Configuration(format!("CLOID must be a valid 16-byte hex value: {err}"))
    })
}

async fn lookup_order(
    client: &HttpClient,
    user: Address,
    identifier: &OrderIdentifier,
) -> Result<BasicOrder, CliError> {
    let order = client
        .order_status(user, identifier.to_status_identifier())
        .await
        .map_err(map_api_error)?
        .ok_or_else(|| CliError::Unsupported(format!("unknown order {}", identifier.display())))?;
    Ok(order.order)
}

async fn asset_index_for_order(
    client: &HttpClient,
    resolver: &AssetResolver,
    order: &BasicOrder,
) -> Result<usize, CliError> {
    if let Some(asset) = parse_internal_asset_id(&order.coin) {
        if let Some(spot_asset) = resolver.spot_asset_index_for_internal_id(asset) {
            return Ok(spot_asset);
        }
        return Ok(asset);
    }

    if let AssetQuery::Outcome(notation) = parse_asset_query(&order.coin) {
        let notation = crate::commands::outcomes::parse_outcome_notation(&notation)?;
        let asset_id = crate::commands::outcomes::outcome_asset_id(notation.encoding)?;
        return usize::try_from(asset_id).map_err(|_| {
            CliError::Unsupported("outcome asset id is outside supported bounds".to_string())
        });
    }

    match resolver.resolve(&order.coin) {
        Ok(ResolvedAsset::Perp { index, .. } | ResolvedAsset::Spot { index, .. }) => Ok(index),
        Err(err) => match parse_asset_query(&order.coin) {
            AssetQuery::Hip3 { dex, token } => {
                match resolve_hip3_tradable_asset(client, &order.coin, &dex, &token).await? {
                    TradableAsset::Perp { index, .. } => Ok(index),
                    TradableAsset::Spot { .. } | TradableAsset::Outcome { .. } => Err(err),
                }
            }
            _ => Err(err),
        },
    }
}

fn filter_orders_by_coin(orders: Vec<BasicOrder>, coin: Option<&str>) -> Vec<BasicOrder> {
    let Some(coin) = coin else {
        return orders;
    };
    orders
        .into_iter()
        .filter(|order| order.coin.eq_ignore_ascii_case(coin))
        .collect()
}

fn resolve_cancel_all_coin_filter(
    resolver: &AssetResolver,
    coin: Option<&str>,
    dex: Option<&str>,
) -> Result<Option<String>, CliError> {
    let Some(coin) = coin else {
        return Ok(None);
    };
    if let AssetQuery::Outcome(notation) = parse_asset_query(coin) {
        let notation = crate::commands::outcomes::parse_outcome_notation(&notation)?;
        return Ok(Some(format!("#{}", notation.encoding)));
    }

    if let Some(asset) = parse_internal_asset_id(coin) {
        return Ok(Some(format!("@{asset}")));
    }

    let query = qualify_dex_asset(dex, coin);
    match resolver.resolve(&query) {
        Ok(ResolvedAsset::Perp { name, dex, .. }) => {
            Ok(Some(dex.map(|dex| format!("{dex}:{name}")).unwrap_or(name)))
        }
        Ok(ResolvedAsset::Spot { index, .. }) => Ok(Some(spot_mid_key(index))),
        Err(err) => match parse_asset_query(&query) {
            AssetQuery::Hip3 { dex, token } => Ok(Some(format!("{dex}:{token}"))),
            _ => Err(err),
        },
    }
}

fn parse_internal_asset_id(input: &str) -> Option<usize> {
    input
        .trim()
        .strip_prefix('@')
        .and_then(|value| value.parse::<usize>().ok())
}

fn qualify_dex_asset(dex: Option<&str>, coin: &str) -> String {
    match dex {
        Some(dex) if !coin.contains(':') => format!("{dex}:{coin}"),
        _ => coin.to_string(),
    }
}

fn order_statuses_from_response(
    response: serde_json::Value,
    action: &'static str,
) -> Result<Vec<OrderResponseStatus>, CliError> {
    #[derive(Deserialize)]
    struct StatusesResponse {
        statuses: Vec<OrderResponseStatus>,
    }

    #[derive(Deserialize)]
    #[serde(tag = "type", content = "data", rename_all = "camelCase")]
    enum TaggedResponse {
        Cancel(StatusesResponse),
        Order(StatusesResponse),
    }

    if let Ok(parsed) = serde_json::from_value::<StatusesResponse>(response.clone()) {
        return Ok(parsed.statuses);
    }

    serde_json::from_value::<TaggedResponse>(response)
        .map(|parsed| match parsed {
            TaggedResponse::Cancel(data) | TaggedResponse::Order(data) => data.statuses,
        })
        .map_err(|err| {
            CliError::Internal(anyhow::anyhow!(
                "{action}: unexpected response shape: {err}"
            ))
        })
}

fn ensure_action_statuses_ok(
    statuses: &[OrderResponseStatus],
    action: &'static str,
) -> Result<usize, CliError> {
    for status in statuses {
        if let OrderResponseStatus::Error(err) = status {
            return Err(CliError::Unsupported(format!("{action} rejected: {err}")));
        }
    }
    Ok(statuses.len())
}

fn modification_result_oid(statuses: &[OrderResponseStatus]) -> Option<u64> {
    statuses.iter().find_map(|status| match status {
        OrderResponseStatus::Resting { oid, .. } | OrderResponseStatus::Filled { oid, .. } => {
            Some(*oid)
        }
        OrderResponseStatus::Success | OrderResponseStatus::Error(_) => None,
    })
}

fn order_type_placement(
    order: &BasicOrder,
    trigger_price: Option<Decimal>,
    legacy_price: Option<Decimal>,
) -> Result<OrderTypePlacement, CliError> {
    match order.order_type {
        OrderType::StopMarket => Ok(OrderTypePlacement::Trigger {
            is_market: true,
            trigger_px: trigger_price.or(legacy_price).unwrap_or(order.limit_px),
            tpsl: TpSl::Sl,
        }),
        OrderType::StopLimit => Ok(OrderTypePlacement::Trigger {
            is_market: false,
            trigger_px: require_modify_trigger_price(trigger_price, "stop-limit")?,
            tpsl: TpSl::Sl,
        }),
        OrderType::TakeProfitMarket => Ok(OrderTypePlacement::Trigger {
            is_market: true,
            trigger_px: trigger_price.or(legacy_price).unwrap_or(order.limit_px),
            tpsl: TpSl::Tp,
        }),
        OrderType::TakeProfitLimit => Ok(OrderTypePlacement::Trigger {
            is_market: false,
            trigger_px: require_modify_trigger_price(trigger_price, "take-limit")?,
            tpsl: TpSl::Tp,
        }),
        OrderType::Trigger => Ok(OrderTypePlacement::Trigger {
            is_market: true,
            trigger_px: trigger_price.or(legacy_price).unwrap_or(order.limit_px),
            tpsl: TpSl::Sl,
        }),
        OrderType::Limit | OrderType::Market => {
            if trigger_price.is_some() {
                return Err(CliError::Configuration(
                    "orders modify --trigger-price only applies to trigger orders".to_string(),
                ));
            }
            Ok(OrderTypePlacement::Limit {
                tif: order.tif.unwrap_or(TimeInForce::Gtc),
            })
        }
    }
}

fn require_modify_trigger_price(
    trigger_price: Option<Decimal>,
    order_type: &'static str,
) -> Result<Decimal, CliError> {
    trigger_price.ok_or_else(|| {
        CliError::Configuration(format!(
            "orders modify requires --trigger-price when modifying {order_type} orders so the trigger price is preserved"
        ))
    })
}

fn confirm_cancel_all(coin: Option<&str>, format: OutputFormat) -> Result<bool, CliError> {
    let mut stderr = io::stderr();
    let prompt = match coin {
        Some(coin) => format!("Cancel all {coin} orders? [y/N] "),
        None => "Cancel all orders? [y/N] ".to_string(),
    };
    write!(stderr, "{}", warning_prompt(&prompt, format))
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    stderr
        .flush()
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    Ok(is_yes(input.trim()))
}

async fn market_mid_price(client: &HttpClient, lookup: &MidLookup) -> Result<Decimal, CliError> {
    let mids = client
        .all_mids(lookup.dex.clone())
        .await
        .map_err(map_api_error)?;
    mids.get(&lookup.coin).copied().ok_or_else(|| {
        CliError::Unavailable(format!(
            "mid price for {} was not available. Retry after market data refresh.",
            lookup.original_coin
        ))
    })
}

fn market_limit_price(
    mid: Decimal,
    side: OrderSide,
    max_slippage_bps: u16,
    price_tick: PriceTick,
) -> Result<Decimal, CliError> {
    let multiplier = match side {
        OrderSide::Buy => {
            Decimal::from(BPS_DENOMINATOR + i64::from(max_slippage_bps))
                / Decimal::from(BPS_DENOMINATOR)
        }
        OrderSide::Sell => {
            Decimal::from(BPS_DENOMINATOR - i64::from(max_slippage_bps))
                / Decimal::from(BPS_DENOMINATOR)
        }
    };
    let limit_px = mid * multiplier;
    let sdk_side = if side.is_buy() { Side::Bid } else { Side::Ask };
    price_tick
        .round_by_side(sdk_side, limit_px, false)
        .ok_or_else(|| {
            CliError::Configuration(format!(
                "derived market order price {limit_px} could not be rounded to a valid tick"
            ))
        })
}

fn confirm_mainnet_order(prepared: &PreparedOrder, format: OutputFormat) -> Result<bool, CliError> {
    let mut stderr = io::stderr();
    let amount = prepared
        .amount
        .map(|amount| format!(", amount {amount} {}", prepared.amount_unit))
        .unwrap_or_default();
    let trigger = prepared
        .trigger_price
        .map(|trigger_price| format!(", trigger price {trigger_price}"))
        .unwrap_or_default();
    let reduce_only = if prepared.request.reduce_only {
        "reduce-only"
    } else {
        "not reduce-only"
    };
    let warning = format!(
        "Mainnet order confirmation required: place {} {} order for {} {} at price {}{}{}, {}.",
        prepared.side,
        prepared.order_type,
        prepared.size,
        prepared.coin,
        prepared.price,
        trigger,
        amount,
        reduce_only
    );
    writeln!(stderr, "{}", warning_prompt(&warning, format))
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    if let Some(slippage_warning) = &prepared.warning {
        writeln!(stderr, "{}", warning_prompt(slippage_warning, format))
            .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    }
    write!(
        stderr,
        "{}",
        warning_prompt("Place this mainnet order? [y/N] ", format)
    )
    .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    stderr
        .flush()
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    Ok(is_yes(input.trim()))
}

fn confirm_mainnet_order_batch(
    label: &str,
    prepared: &PreparedOrderBatch,
    format: OutputFormat,
) -> Result<bool, CliError> {
    let mut stderr = io::stderr();
    let prompt = format!(
        "Submit {count} {label} order(s) on mainnet? [y/N] ",
        count = prepared.legs.len()
    );
    write!(stderr, "{}", warning_prompt(&prompt, format))
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    stderr
        .flush()
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    Ok(is_yes(input.trim()))
}

fn confirm_mainnet_tpsl_batch(
    prepared: &PreparedTpslBatch,
    format: OutputFormat,
) -> Result<bool, CliError> {
    let mut stderr = io::stderr();
    let parent = prepared
        .legs
        .iter()
        .find(|leg| leg.leg == TpslLegKind::Parent);
    let summary = if let Some(parent) = parent {
        format!(
            "Mainnet TP/SL confirmation required: place {} parent order for {} {} at price {} with {} grouped child order(s) using {}.",
            parent.side,
            parent.size,
            parent.coin,
            parent.price,
            prepared.legs.len().saturating_sub(1),
            prepared.grouping
        )
    } else {
        let first = prepared.legs.first().ok_or_else(|| {
            CliError::Configuration("TP/SL batch requires at least one leg".to_string())
        })?;
        format!(
            "Mainnet TP/SL confirmation required: place {} grouped TP/SL order(s) for {} using {}.",
            prepared.legs.len(),
            first.coin,
            prepared.grouping
        )
    };
    writeln!(stderr, "{}", warning_prompt(&summary, format))
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    for leg in &prepared.legs {
        if let Some(warning) = &leg.warning {
            writeln!(stderr, "{}", warning_prompt(warning, format))
                .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
        }
    }
    write!(
        stderr,
        "{}",
        warning_prompt("Place this mainnet TP/SL batch? [y/N] ", format)
    )
    .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    stderr
        .flush()
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    Ok(is_yes(input.trim()))
}

fn confirm_mainnet_twap(
    coin: &str,
    args: &TwapCreateArgs,
    format: OutputFormat,
) -> Result<bool, CliError> {
    let mut stderr = io::stderr();
    let warning = format!(
        "Mainnet TWAP confirmation required: create {} TWAP for {} {} over {} seconds on mainnet.",
        args.side, args.size, coin, args.duration
    );
    writeln!(stderr, "{}", warning_prompt(&warning, format))
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    write!(
        stderr,
        "{}",
        warning_prompt("Create this mainnet TWAP? [y/N] ", format)
    )
    .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    stderr
        .flush()
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    Ok(is_yes(input.trim()))
}

fn warning_prompt(prompt: &str, format: OutputFormat) -> String {
    if format == OutputFormat::Pretty {
        output::colors::yellow(prompt)
    } else {
        prompt.to_string()
    }
}

fn is_yes(input: &str) -> bool {
    matches!(input.to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{AssetMetadata, AssetResolver, PerpAsset, SpotAsset};

    fn resolver() -> AssetResolver {
        AssetResolver::new(AssetMetadata::from_assets(
            vec![
                PerpAsset {
                    name: "BTC".to_string(),
                    index: 0,
                    dex: None,
                    sz_decimals: 5,
                    collateral: "USDC".to_string(),
                },
                PerpAsset::hip3("testdex", "HYPE", 11_100),
            ],
            vec![SpotAsset {
                symbol: "HYPE/USDC".to_string(),
                index: 11_035,
                base: "HYPE".to_string(),
                quote: "USDC".to_string(),
                base_sz_decimals: 2,
            }],
        ))
    }

    fn base_args() -> CreateArgs {
        CreateArgs {
            coin: "BTC".to_string(),
            dex: None,
            side: OrderSide::Buy,
            price: Some(Decimal::from(50_000)),
            trigger_price: None,
            size: Some(Decimal::new(1, 1)),
            amount: None,
            order_type: CreateOrderType::Limit,
            tif: TifArg::Gtc,
            reduce_only: false,
            max_slippage_bps: DEFAULT_MARKET_ORDER_SLIPPAGE_BPS,
            take_profit: None,
            stop_loss: None,
            grouping: None,
            on_behalf_of: None,
            margin_mode: None,
            builder: None,
            builder_fee_rate: None,
            cloid: None,
            yes: false,
        }
    }

    #[test]
    fn market_limit_price_rounds_buy_up_to_perp_tick() {
        let price = market_limit_price(
            Decimal::new(9_472_033, 2),
            OrderSide::Buy,
            200,
            PriceTick::for_perp(5),
        )
        .unwrap();

        assert_eq!(price, Decimal::from(96_615));
    }

    #[test]
    fn market_limit_price_rounds_sell_down_to_perp_tick() {
        let price = market_limit_price(
            Decimal::new(9_472_033, 2),
            OrderSide::Sell,
            200,
            PriceTick::for_perp(5),
        )
        .unwrap();

        assert_eq!(price, Decimal::from(92_825));
    }

    #[tokio::test]
    async fn limit_order_builds_hypersdk_request() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let prepared = prepare_order(&client, &resolver(), &base_args())
            .await
            .unwrap();

        assert_eq!(prepared.request.asset, 0);
        assert!(prepared.request.is_buy);
        assert_eq!(prepared.request.limit_px, Decimal::from(50_000));
        assert_eq!(prepared.request.sz, Decimal::new(1, 1));
        assert!(!prepared.request.reduce_only);
        assert!(matches!(
            prepared.request.order_type,
            OrderTypePlacement::Limit {
                tif: TimeInForce::Gtc
            }
        ));
    }

    #[tokio::test]
    async fn normal_tpsl_grouping_builds_parent_and_child_orders() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.take_profit = Some(Decimal::from(55_000));
        args.stop_loss = Some(Decimal::from(49_000));
        args.grouping = Some(CreateTpslGroupingArg::NormalTpsl);

        let parent = prepare_order(&client, &resolver(), &args).await.unwrap();
        let prepared = prepare_normal_tpsl_batch(parent, &args).unwrap();
        let json = serde_json::to_value(&prepared.batch).unwrap();

        assert_eq!(prepared.legs.len(), 3);
        assert!(matches!(prepared.batch.grouping, OrderGrouping::NormalTpsl));
        assert_eq!(json["grouping"], "normalTpsl");
        assert_eq!(prepared.legs[0].leg, TpslLegKind::Parent);
        assert!(!prepared.legs[0].reduce_only);
        assert_eq!(prepared.legs[1].leg, TpslLegKind::TakeProfit);
        assert_eq!(prepared.legs[1].side, OrderSide::Sell);
        assert_eq!(prepared.legs[1].size, Decimal::new(1, 1));
        assert!(prepared.legs[1].reduce_only);
        assert!(matches!(
            prepared.legs[1].request.order_type,
            OrderTypePlacement::Trigger {
                is_market: true,
                trigger_px,
                tpsl: TpSl::Tp,
            } if trigger_px == Decimal::from(55_000)
        ));
        assert!(matches!(
            prepared.legs[2].request.order_type,
            OrderTypePlacement::Trigger {
                is_market: true,
                trigger_px,
                tpsl: TpSl::Sl,
            } if trigger_px == Decimal::from(49_000)
        ));
    }

    #[tokio::test]
    async fn normal_tpsl_grouping_accepts_hip3_perp_asset_ids() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.coin = "HYPE".to_string();
        args.dex = Some("testdex".to_string());
        args.take_profit = Some(Decimal::from(55_000));
        args.stop_loss = Some(Decimal::from(49_000));
        args.grouping = Some(CreateTpslGroupingArg::NormalTpsl);

        let parent = prepare_order(&client, &resolver(), &args).await.unwrap();

        assert_eq!(parent.asset_kind, TradableAssetKind::Perp);
        assert!(parent.request.asset >= 10_000);

        let prepared = prepare_normal_tpsl_batch(parent, &args).unwrap();
        let json = serde_json::to_value(&prepared.batch).unwrap();

        assert_eq!(prepared.legs.len(), 3);
        assert_eq!(prepared.legs[0].coin, "testdex:HYPE");
        assert!(matches!(prepared.batch.grouping, OrderGrouping::NormalTpsl));
        assert_eq!(json["grouping"], "normalTpsl");
    }

    #[test]
    fn fallback_collateral_name_prefers_token_indices_over_positions() {
        let tokens = vec![
            SpotMetaTokenName {
                name: "USDC".to_string(),
                index: 0,
            },
            SpotMetaTokenName {
                name: "WRONG".to_string(),
                index: 1,
            },
            SpotMetaTokenName {
                name: "USDH".to_string(),
                index: 42,
            },
        ];

        assert_eq!(collateral_name_from_tokens(&tokens, Some(42)), "USDH");
    }

    #[test]
    fn fallback_collateral_name_falls_back_to_token_position() {
        let tokens = vec![
            SpotMetaTokenName {
                name: "USDC".to_string(),
                index: 0,
            },
            SpotMetaTokenName {
                name: "USDH".to_string(),
                index: 42,
            },
        ];

        assert_eq!(collateral_name_from_tokens(&tokens, Some(1)), "USDH");
    }

    #[test]
    fn position_tpsl_grouping_builds_reduce_only_trigger_legs() {
        let prepared = build_position_tpsl_batch(
            TpslGroupingArg::PositionTpsl,
            "BTC".to_string(),
            0,
            "USDC".to_string(),
            OrderSide::Sell,
            Decimal::new(1, 1),
            Some(Decimal::from(55_000)),
            Some(Decimal::from(49_000)),
            None,
        );
        let json = serde_json::to_value(&prepared.batch).unwrap();

        assert_eq!(prepared.legs.len(), 2);
        assert!(matches!(
            prepared.batch.grouping,
            OrderGrouping::PositionTpsl
        ));
        assert_eq!(json["grouping"], "positionTpsl");
        assert!(prepared.legs.iter().all(|leg| leg.reduce_only));
        assert!(prepared.legs.iter().all(|leg| leg.side == OrderSide::Sell));
        assert_eq!(json["orders"][0]["r"], true);
        assert_eq!(json["orders"][0]["t"]["trigger"]["tpsl"], "tp");
        assert_eq!(json["orders"][1]["t"]["trigger"]["tpsl"], "sl");
    }

    #[test]
    fn position_tpsl_batch_applies_custom_cloid_to_first_child_leg() {
        let cloid = "0x00000000000000000000000000000001"
            .parse::<Cloid>()
            .unwrap();
        let prepared = build_position_tpsl_batch(
            TpslGroupingArg::PositionTpsl,
            "BTC".to_string(),
            0,
            "USDC".to_string(),
            OrderSide::Sell,
            Decimal::new(1, 1),
            Some(Decimal::from(55_000)),
            Some(Decimal::from(49_000)),
            Some(cloid),
        );

        assert_eq!(prepared.legs.len(), 2);
        // Take-profit leg (first child) gets the custom cloid
        assert_eq!(prepared.legs[0].request.cloid, cloid);
        assert_eq!(prepared.legs[0].leg, TpslLegKind::TakeProfit);
        // Stop-loss leg (second child) gets default to avoid duplicates
        assert_eq!(prepared.legs[1].request.cloid, Cloid::default());
        assert_eq!(prepared.legs[1].leg, TpslLegKind::StopLoss);
    }

    #[test]
    fn position_tpsl_batch_applies_cloid_to_sl_when_tp_absent() {
        let cloid = "0x00000000000000000000000000000001"
            .parse::<Cloid>()
            .unwrap();
        let prepared = build_position_tpsl_batch(
            TpslGroupingArg::PositionTpsl,
            "BTC".to_string(),
            0,
            "USDC".to_string(),
            OrderSide::Sell,
            Decimal::new(1, 1),
            None, // no take-profit
            Some(Decimal::from(49_000)),
            Some(cloid),
        );

        assert_eq!(prepared.legs.len(), 1);
        // When only stop-loss is present, it gets the custom cloid
        assert_eq!(prepared.legs[0].request.cloid, cloid);
        assert_eq!(prepared.legs[0].leg, TpslLegKind::StopLoss);
    }

    #[test]
    fn validate_tpsl_args_rejects_invalid_cloid() {
        let mut args = TpslArgs {
            coin: "BTC".to_string(),
            dex: None,
            take_profit: Some(Decimal::from(55_000)),
            stop_loss: None,
            grouping: PositionTpslGroupingArg::PositionTpsl,
            side: Some(OrderSide::Sell),
            size: Some(Decimal::new(1, 1)),
            margin_mode: None,
            yes: false,
            cloid: Some("not-a-hex-value".to_string()),
        };
        let err = validate_tpsl_args(&args).unwrap_err();
        assert!(
            err.to_string().contains("CLOID must be a 0x-prefixed"),
            "expected cloid validation error, got: {err}"
        );

        args.cloid = Some("0xDEADBEEF".to_string());
        let err = validate_tpsl_args(&args).unwrap_err();
        assert!(
            err.to_string().contains("CLOID must be exactly 16 bytes"),
            "expected cloid length error, got: {err}"
        );
    }

    #[test]
    fn tpsl_grouping_validation_rejects_missing_legs() {
        let mut create_args = base_args();
        create_args.grouping = Some(CreateTpslGroupingArg::NormalTpsl);
        let err = validate_create_args(&create_args).unwrap_err();
        assert!(err.to_string().contains("--grouping requires"));

        create_args.take_profit = Some(Decimal::from(55_000));
        create_args.grouping = None;
        let err = validate_create_args(&create_args).unwrap_err();
        assert!(err.to_string().contains("normal-tpsl"));

        let tpsl_args = TpslArgs {
            coin: "BTC".to_string(),
            dex: None,
            take_profit: None,
            stop_loss: None,
            grouping: PositionTpslGroupingArg::PositionTpsl,
            side: None,
            size: None,
            margin_mode: None,
            yes: false,
            cloid: None,
        };
        let err = validate_tpsl_args(&tpsl_args).unwrap_err();
        assert!(
            err.to_string()
                .contains("requires --take-profit or --stop-loss")
        );
    }

    #[tokio::test]
    async fn spot_limit_order_builds_hypersdk_request() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.coin = "HYPE/USDC".to_string();
        args.price = Some(Decimal::new(9_690, 2));
        args.size = Some(Decimal::new(20, 2));
        args.tif = TifArg::Ioc;

        let prepared = prepare_order(&client, &resolver(), &args).await.unwrap();

        assert_eq!(prepared.coin, "HYPE/USDC");
        assert_eq!(prepared.request.asset, 11_035);
        assert!(prepared.request.is_buy);
        assert_eq!(prepared.request.limit_px, Decimal::new(9_690, 2));
        assert_eq!(prepared.request.sz, Decimal::new(20, 2));
        assert!(!prepared.request.reduce_only);
        assert!(matches!(
            prepared.request.order_type,
            OrderTypePlacement::Limit {
                tif: TimeInForce::Ioc
            }
        ));
    }

    #[tokio::test]
    async fn limit_order_reduce_only_builds_hypersdk_request() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.reduce_only = true;

        let prepared = prepare_order(&client, &resolver(), &args).await.unwrap();

        assert!(prepared.request.reduce_only);
        assert!(matches!(
            prepared.request.order_type,
            OrderTypePlacement::Limit {
                tif: TimeInForce::Gtc
            }
        ));
    }

    #[tokio::test]
    async fn market_order_reduce_only_builds_hypersdk_request() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.order_type = CreateOrderType::Market;
        args.price = None;
        args.size = None;
        args.amount = Some(Decimal::from(50));
        args.reduce_only = true;

        let prepared = prepare_order(&client, &resolver(), &args).await.unwrap();

        assert!(prepared.request.reduce_only);
        assert!(matches!(
            prepared.request.order_type,
            OrderTypePlacement::Limit {
                tif: TimeInForce::FrontendMarket
            }
        ));
    }

    #[test]
    fn scale_validation_rejects_unbounded_or_ambiguous_ladders() {
        let mut args = ScaleArgs {
            coin: "BTC".to_string(),
            dex: None,
            side: OrderSide::Buy,
            start_price: Decimal::from(50_000),
            end_price: Decimal::from(50_000),
            total_size: Decimal::new(1, 1),
            order_count: 2,
            tif: TifArg::Gtc,
            reduce_only: false,
            on_behalf_of: None,
            margin_mode: None,
            yes: false,
        };

        let err = validate_scale_args(&args).unwrap_err();
        assert!(err.to_string().contains("different start and end"));

        args.end_price = Decimal::from(49_000);
        args.order_count = MAX_BATCH_ORDER_COUNT + 1;
        let err = validate_scale_args(&args).unwrap_err();
        assert!(err.to_string().contains("at most"));
    }

    #[test]
    fn scale_sizes_rejects_non_even_distribution() {
        let err = scale_sizes(Decimal::new(1, 2), 3).unwrap_err();
        assert!(err.to_string().contains("evenly distributed"));
    }

    #[tokio::test]
    async fn spot_reduce_only_orders_are_rejected_before_submission() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.coin = "HYPE/USDC".to_string();
        args.reduce_only = true;

        let err = prepare_order(&client, &resolver(), &args)
            .await
            .unwrap_err();

        assert_eq!(err.exit_code(), 13);
        assert!(err.to_string().contains("--reduce-only"));
        assert!(err.to_string().contains("perpetual markets only"));
    }

    #[tokio::test]
    async fn spot_trigger_orders_are_rejected_before_submission() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.coin = "HYPE/USDC".to_string();
        args.order_type = CreateOrderType::StopLimit;
        args.trigger_price = Some(Decimal::from(48_000));

        let err = prepare_order(&client, &resolver(), &args)
            .await
            .unwrap_err();

        assert_eq!(err.exit_code(), 13);
        assert!(
            err.to_string()
                .contains("trigger orders currently support perpetual markets only")
        );
    }

    #[test]
    fn spot_trigger_orders_are_rejected_after_asset_resolution() {
        let mut args = base_args();
        args.coin = "HYPE/USDC".to_string();
        args.order_type = CreateOrderType::StopLimit;
        args.trigger_price = Some(Decimal::from(48_000));
        let asset = ResolvedAsset::Spot {
            symbol: "HYPE/USDC".to_string(),
            index: 11_035,
            base: "HYPE".to_string(),
            quote: "USDC".to_string(),
            base_sz_decimals: 2,
        };

        let err = validate_create_resolved_asset(&args, &asset).unwrap_err();

        assert_eq!(err.exit_code(), 13);
        assert!(
            err.to_string()
                .contains("trigger orders currently support perpetual markets only")
        );
    }

    #[tokio::test]
    async fn stop_loss_builds_trigger_order() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.order_type = CreateOrderType::StopLoss;
        let prepared = prepare_order(&client, &resolver(), &args).await.unwrap();

        assert!(matches!(
            prepared.request.order_type,
            OrderTypePlacement::Trigger {
                is_market: true,
                trigger_px,
                tpsl: TpSl::Sl,
            } if trigger_px == Decimal::from(50_000)
        ));
        assert_eq!(prepared.trigger_price, Some(Decimal::from(50_000)));
        assert!(prepared.request.reduce_only);
    }

    #[tokio::test]
    async fn take_profit_builds_reduce_only_trigger_order() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.order_type = CreateOrderType::TakeProfit;
        let prepared = prepare_order(&client, &resolver(), &args).await.unwrap();

        assert!(matches!(
            prepared.request.order_type,
            OrderTypePlacement::Trigger {
                is_market: true,
                trigger_px,
                tpsl: TpSl::Tp,
            } if trigger_px == Decimal::from(50_000)
        ));
        assert!(prepared.request.reduce_only);
    }

    #[tokio::test]
    async fn orders_trigger_limit_stop_limit_without_reduce_only_builds_limit_trigger_payload() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.side = OrderSide::Sell;
        args.order_type = CreateOrderType::StopLimit;
        args.trigger_price = Some(Decimal::from(49_000));
        args.price = Some(Decimal::from(48_750));

        let prepared = prepare_order(&client, &resolver(), &args).await.unwrap();

        assert_eq!(prepared.request.limit_px, Decimal::from(48_750));
        assert!(matches!(
            prepared.request.order_type,
            OrderTypePlacement::Trigger {
                is_market: false,
                trigger_px,
                tpsl: TpSl::Sl,
            } if trigger_px == Decimal::from(49_000)
        ));
        assert!(!prepared.request.reduce_only);
    }

    #[tokio::test]
    async fn orders_trigger_limit_take_limit_with_reduce_only_builds_limit_trigger_payload() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.side = OrderSide::Sell;
        args.order_type = CreateOrderType::TakeLimit;
        args.trigger_price = Some(Decimal::from(55_000));
        args.price = Some(Decimal::from(54_750));
        args.reduce_only = true;

        let prepared = prepare_order(&client, &resolver(), &args).await.unwrap();

        assert_eq!(prepared.request.limit_px, Decimal::from(54_750));
        assert!(matches!(
            prepared.request.order_type,
            OrderTypePlacement::Trigger {
                is_market: false,
                trigger_px,
                tpsl: TpSl::Tp,
            } if trigger_px == Decimal::from(55_000)
        ));
        assert!(prepared.request.reduce_only);
    }

    #[test]
    fn orders_trigger_limit_requires_trigger_price() {
        let mut args = base_args();
        args.order_type = CreateOrderType::StopLimit;
        args.trigger_price = None;

        let err = validate_create_args(&args).unwrap_err();

        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("--trigger-price"));
    }

    #[test]
    fn stop_loss_keeps_legacy_price_as_market_trigger_price() {
        let mut args = base_args();
        args.order_type = CreateOrderType::StopLoss;
        args.trigger_price = None;

        validate_create_args(&args).unwrap();
    }

    #[tokio::test]
    async fn rejects_negative_price_before_api_or_auth() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.price = Some(Decimal::from(-1));
        let err = prepare_order(&client, &resolver(), &args)
            .await
            .unwrap_err();

        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("price must be positive"));
    }

    #[test]
    fn signed_exchange_user_state_rejections_are_not_internal_errors() {
        let err = actions::map_exchange_error(
            "User or API Wallet 0x1111111111111111111111111111111111111111 does not exist."
                .to_string(),
            "order placement failed",
        );

        assert_eq!(err.exit_code(), 13);
        assert!(err.to_string().contains("order placement failed"));
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn tpsl_response_parser_accepts_waiting_children() {
        let response = serde_json::json!({
            "type": "order",
            "data": {
                "statuses": [
                    {"resting": {"oid": 12345}},
                    "waitingForFill",
                    "waitingForTrigger"
                ]
            }
        });

        let statuses = parse_tpsl_order_statuses(response).unwrap();

        assert!(matches!(
            statuses.as_slice(),
            [
                TpslResponseStatus::Resting { oid: 12345 },
                TpslResponseStatus::WaitingForFill,
                TpslResponseStatus::WaitingForTrigger
            ]
        ));
    }
}
