//! Authenticated order management commands.

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

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

use crate::assets::asset_id::{
    outcome_coin, parse_spot_coin_key, spot_coin_key, spot_internal_coin_key,
};
use crate::assets::{
    AssetQuery, AssetResolver, Market, OutcomeAsset, parse_asset_query, qualify_dex_asset,
};
use crate::commands::{actions, builder};
use crate::errors::from_sdk;

use crate::errors::CliError;
use crate::http_api::post_info_json;
use crate::input_hardening::{FilePolicy, read_json_file};
use crate::invocation::Invocation;
use crate::mode::Mode;
use crate::output::{self, OutputFormat, TableData};
use crate::prompt;
use crate::response_sanitization::labelled_untrusted_text;
use crate::signing::SelectedSigner;

const DEFAULT_MARKET_ORDER_SLIPPAGE_BPS: u16 = 500;
const MIN_MARKET_ORDER_SLIPPAGE_BPS: u16 = 1;
const MAX_MARKET_ORDER_SLIPPAGE_BPS: u16 = 1_000;
const MAX_BATCH_ORDER_COUNT: usize = 500;
const BPS_DENOMINATOR: i64 = 10_000;

mod args;
mod bracket;
mod chase;
mod hip3;
mod planning;
mod queries;
mod rendering;
mod trailing_stop;
mod validation;

pub use bracket::{bracket, bracket_dry_run_plan, validate_bracket_args};
pub use chase::{chase, chase_dry_run_plan};
pub use hip3::{
    Hip3MarginPlan, execute_hip3_margin_transfer, hip3_dex_from_coin, plan_hip3_margin_transfer,
};
pub use trailing_stop::{
    TrailingStopArgs, trailing_stop, trailing_stop_dry_run_plan, validate_trailing_stop_args,
};

pub use args::*;
use planning::{
    CreateOrderSubmission, ScheduleCancelPlan, prepare_batch_create_order_plan,
    prepare_cancel_all_orders_plan, prepare_cancel_order_plan, prepare_create_order_plan,
    prepare_modify_order_plan, prepare_position_tpsl_batch, prepare_scale_batch,
    prepare_schedule_cancel_plan, prepare_twap_cancel_plan, prepare_twap_create_plan,
};
pub use planning::{
    OrderDryRunPlan, batch_create_dry_run_args, batch_create_dry_run_plan, cancel_all_dry_run_plan,
    cancel_dry_run_plan, create_dry_run_args, create_dry_run_plan, create_dry_run_preview,
    modify_dry_run_plan, scale_dry_run_args, scale_dry_run_plan, schedule_cancel_dry_run_plan,
    tpsl_dry_run_plan, tpsl_dry_run_preview, twap_cancel_dry_run_plan, twap_create_dry_run_plan,
};
#[cfg(test)]
use planning::{build_position_tpsl_batch, prepare_normal_tpsl_batch, prepare_order};
pub use queries::{history, open, open_snapshot, status};
pub use rendering::OrderListOutput;
use rendering::{
    CancelAllSummary, CancelAllSummaryOutput, CancelConfirmation, CancelConfirmationOutput,
    CancelLeg, CancelLegsOutput, ModifyConfirmation, ModifyConfirmationOutput, OrderConfirmation,
    OrderConfirmationOutput, OrderListRow, PartialBatchOutput, ScheduleCancelConfirmation,
    ScheduleCancelOutput, TpslOrderConfirmation, TpslOrderConfirmationOutput, TpslResponseStatus,
    TwapCancelConfirmation, TwapCancelOutput, TwapCreateConfirmation, TwapCreateOutput,
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

#[derive(Clone, Copy)]
pub struct OrderExecutionContext<'a> {
    pub submission: OrderSubmissionContext<'a>,
    pub client: &'a HttpClient,
    pub resolver: &'a AssetResolver,
}

#[derive(Clone, Copy)]
pub struct OrderSubmissionContext<'a> {
    pub inv: &'a Invocation,
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
    emit_output: bool,
) -> Result<(), anyhow::Error> {
    let plan = prepare_create_order_plan(context.client, context.resolver, args).await?;
    let user = vault_address.unwrap_or_else(|| context.submission.signer.query_address());
    match plan.submission {
        CreateOrderSubmission::NormalTpsl(prepared) => {
            let (notional, coin) = prepared
                .legs
                .first()
                .map(|leg| (leg.size * leg.price, leg.coin.clone()))
                .unwrap_or_default();
            let helper = plan_hip3_helper(context, user, &coin, notional).await?;
            if context.submission.require_mainnet_confirmation && !args.yes {
                warn_hip3_margin_helper(&helper)?;
                let target = ConsentTarget::new(context.submission, vault_address, None);
                if !confirm_mainnet_tpsl_batch(&prepared, &target)? {
                    return Err(CliError::Configuration(
                        "Mainnet TP/SL confirmation required; order placement cancelled. Rerun with --yes for deliberate automation."
                            .to_string(),
                    )
                    .into());
                }
            }
            execute_hip3_helper(context, user, &helper).await?;

            let statuses = place_tpsl_batch(
                context.submission.api_base_url,
                context.submission.chain,
                context.submission.signer,
                prepared.batch.clone(),
                vault_address,
            )
            .await?;
            let rows = tpsl_confirmation_rows(&prepared, statuses)?;
            finish_tpsl_batch(rows, context.submission.inv, emit_output)
        }
        CreateOrderSubmission::Single(prepared) => {
            let statuses =
                place_prepared_single(context, prepared.clone(), args, vault_address).await?;
            let rows = statuses
                .into_iter()
                .map(|status| OrderConfirmation::from_status(&prepared, status))
                .collect::<Result<Vec<_>, _>>()?;
            emit_order_output(
                &OrderConfirmationOutput { rows },
                context.submission.inv,
                emit_output,
            );
            Ok(())
        }
    }
}

/// Plan the HIP-3 margin helper transfer for a prepared order, if one is needed.
async fn plan_hip3_helper(
    context: OrderExecutionContext<'_>,
    user: Address,
    coin: &str,
    notional: Decimal,
) -> Result<Option<Hip3MarginPlan>, CliError> {
    let Some(dex) = hip3_dex_from_coin(coin, None) else {
        return Ok(None);
    };
    plan_hip3_margin_transfer(
        context.client,
        context.submission.api_base_url,
        user,
        Some(dex.as_str()),
        notional,
    )
    .await
}

/// Disclose a planned HIP-3 margin helper transfer before order confirmation.
fn warn_hip3_margin_helper(helper: &Option<Hip3MarginPlan>) -> Result<(), CliError> {
    if let Some(plan) = helper {
        let mut stderr = io::stderr();
        writeln!(
            stderr,
            "{}",
            prompt::warning(&format!(
                "This order requires a helper transfer of {} USDC from perp to dex:{} before placement.",
                plan.amount, plan.dest_dex
            ))
        )
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    }
    Ok(())
}

/// Execute a planned HIP-3 margin helper transfer after confirmation.
async fn execute_hip3_helper(
    context: OrderExecutionContext<'_>,
    user: Address,
    helper: &Option<Hip3MarginPlan>,
) -> Result<(), anyhow::Error> {
    if let Some(plan) = helper {
        execute_hip3_margin_transfer(
            context.submission.inv.mode(),
            context.submission.inv.remote()?,
            context.submission.signer,
            user,
            plan,
        )
        .await?;
    }
    Ok(())
}

/// Submit a single prepared order, running the HIP-3 margin helper after
/// confirmation and before placement. Returns the exchange order statuses.
async fn place_prepared_single(
    context: OrderExecutionContext<'_>,
    prepared: PreparedOrder,
    args: &CreateArgs,
    vault_address: Option<Address>,
) -> Result<Vec<OrderResponseStatus>, anyhow::Error> {
    let user = vault_address.unwrap_or_else(|| context.submission.signer.query_address());
    let helper = plan_hip3_helper(
        context,
        user,
        &prepared.coin,
        prepared.size * prepared.price,
    )
    .await?;
    if context.submission.require_mainnet_confirmation && !args.yes {
        warn_hip3_margin_helper(&helper)?;
        let target =
            ConsentTarget::new(context.submission, vault_address, prepared.builder.as_ref());
        if !confirm_mainnet_order(&prepared, &target)? {
            return Err(CliError::Configuration(
                "Mainnet order confirmation required; order placement cancelled. Rerun with --yes for deliberate automation."
                    .to_string(),
            )
            .into());
        }
    }
    execute_hip3_helper(context, user, &helper).await?;

    let batch = BatchOrder {
        orders: vec![prepared.request.clone()],
        grouping: OrderGrouping::Na,
        // The CLI sends builder fees through `place_builder_order_batch_raw`, which
        // serializes its own `{ b, f }` object; the SDK field stays unset.
        builder: None,
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
    Ok(statuses)
}

/// Place a generated scale order batch.
pub async fn scale(
    context: OrderExecutionContext<'_>,
    args: &ScaleArgs,
    vault_address: Option<Address>,
) -> Result<(), anyhow::Error> {
    let prepared = prepare_scale_batch(context.client, context.resolver, args).await?;
    if context.submission.require_mainnet_confirmation && !args.yes {
        let target = ConsentTarget::new(context.submission, vault_address, None);
        if !confirm_mainnet_order_batch("scale", &prepared, &target)? {
            return Err(CliError::Configuration(
                "Mainnet scale order confirmation required; order placement cancelled. Rerun with --yes for deliberate automation."
                    .to_string(),
            )
            .into());
        }
    }

    let rows = place_order_batch(context, prepared, vault_address).await?;
    finish_order_batch(rows, context.submission.inv)
}

/// Place an explicit JSON order batch.
pub async fn batch_create(
    context: OrderExecutionContext<'_>,
    args: &BatchCreateArgs,
    orders: Vec<BatchCreateOrder>,
    vault_address: Option<Address>,
) -> Result<(), anyhow::Error> {
    let plan =
        prepare_batch_create_order_plan(context.client, context.resolver, args, orders).await?;
    let prepared = plan.prepared;
    if context.submission.require_mainnet_confirmation && !args.yes {
        let target = ConsentTarget::new(context.submission, vault_address, None);
        if !confirm_mainnet_order_batch("batch-create", &prepared, &target)? {
            return Err(CliError::Configuration(
                "Mainnet batch order confirmation required; order placement cancelled. Rerun with --yes for deliberate automation."
                    .to_string(),
            )
            .into());
        }
    }

    let rows = place_order_batch(context, prepared, vault_address).await?;
    finish_order_batch(rows, context.submission.inv)
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
        return Err(status_count_mismatch(statuses.len(), prepared.legs.len()));
    }

    Ok(prepared
        .legs
        .iter()
        .zip(statuses)
        .map(|(leg, status)| OrderConfirmation::from_leg_status(&leg.order, status))
        .collect())
}

/// Place position-attached take-profit and/or stop-loss trigger orders.
pub async fn tpsl(
    context: OrderExecutionContext<'_>,
    args: &TpslArgs,
    vault_address: Option<Address>,
    emit_output: bool,
) -> Result<(), anyhow::Error> {
    let user = vault_address.unwrap_or_else(|| context.submission.signer.query_address());
    let prepared =
        prepare_position_tpsl_batch(context.client, context.resolver, user, args).await?;
    if context.submission.require_mainnet_confirmation && !args.yes {
        let target = ConsentTarget::new(context.submission, vault_address, None);
        if !confirm_mainnet_tpsl_batch(&prepared, &target)? {
            return Err(CliError::Configuration(
                "Mainnet TP/SL confirmation required; order placement cancelled. Rerun with --yes for deliberate automation."
                    .to_string(),
            )
            .into());
        }
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
    finish_tpsl_batch(rows, context.submission.inv, emit_output)
}

fn emit_order_output(data: &dyn TableData, inv: &Invocation, emit_output: bool) {
    if emit_output {
        inv.print(data);
    }
}

/// Sanitized exchange errors of the rejected legs, in leg order.
fn rejection_errors<'a>(errors: impl IntoIterator<Item = Option<&'a str>>) -> Vec<String> {
    errors.into_iter().flatten().map(str::to_string).collect()
}

/// Error for a batch where the exchange rejected every leg. `failures` are
/// already sanitized and labelled.
fn rejected_all(action: &str, failures: &[String]) -> CliError {
    match failures {
        [one] => CliError::Unsupported(format!("Couldn't {action}. Exchange said: {one}")),
        many => {
            let lines = many
                .iter()
                .enumerate()
                .map(|(index, failure)| format!("  {}. {failure}", index + 1))
                .collect::<Vec<_>>()
                .join("\n");
            CliError::Unsupported(format!("Couldn't {action}. Exchange said:\n{lines}"))
        }
    }
}

/// Report a batch where some legs failed. Prints the per-leg result, then
/// returns the partial-results error (exit 15). When every leg failed there is
/// no partial result, so the rejection is the error.
fn finish_with_failures(
    legs: impl TableData + 'static,
    verb: &str,
    action: &str,
    failures: Vec<String>,
    inv: &Invocation,
) -> Result<(), anyhow::Error> {
    let total = legs.rows().len();
    let failed = failures.len();
    if failed >= total {
        return Err(rejected_all(action, &failures).into());
    }
    let succeeded = total - failed;
    let partial = PartialBatchOutput {
        legs: Box::new(legs),
        legs_key: "orders",
        succeeded,
        failed,
        headline: format!(
            "{verb} {succeeded} of {total} {}. {failed} failed.",
            plural(total, "order", "orders")
        ),
    };
    inv.print(&partial);
    Err(partial.error().into())
}

/// Print a placed batch, or report its failed legs.
fn finish_order_batch(rows: Vec<OrderConfirmation>, inv: &Invocation) -> Result<(), anyhow::Error> {
    let legs = OrderConfirmationOutput { rows };
    let failures = legs.rejection_errors();
    if failures.is_empty() {
        inv.print(&legs);
        return Ok(());
    }
    finish_with_failures(legs, "Placed", "place orders", failures, inv)
}

/// Print a placed TP/SL batch, or report its failed legs. With `emit_output`
/// off (bracket protection) a failed leg is returned as an error instead.
fn finish_tpsl_batch(
    rows: Vec<TpslOrderConfirmation>,
    inv: &Invocation,
    emit_output: bool,
) -> Result<(), anyhow::Error> {
    let legs = TpslOrderConfirmationOutput { rows };
    let failures = legs.rejection_errors();
    if failures.is_empty() {
        emit_order_output(&legs, inv, emit_output);
        return Ok(());
    }
    if !emit_output {
        return Err(rejected_all("place TP/SL orders", &failures).into());
    }
    finish_with_failures(legs, "Placed", "place TP/SL orders", failures, inv)
}

/// Cancel a single open order by exchange OID or client order ID.
pub async fn cancel(
    context: OrderExecutionContext<'_>,
    user: Address,
    args: &CancelArgs,
    vault_address: Option<Address>,
) -> Result<(), anyhow::Error> {
    context
        .submission
        .signer
        .ensure_can_attempt_live_signing()?;
    let plan = prepare_cancel_order_plan(context.client, context.resolver, user, args).await?;
    let nonce = Utc::now().timestamp_millis() as u64;
    let response = actions::send_l1_action_raw(
        context.submission.api_base_url,
        context.submission.chain,
        context.submission.signer,
        plan.action,
        nonce,
        vault_address,
        "cancel failed",
    )
    .await?;
    let statuses = order_statuses_from_response(response, "cancel")?;

    ensure_action_statuses_ok(&statuses, "cancel order")?;
    context.submission.inv.print(&CancelConfirmationOutput {
        rows: vec![CancelConfirmation {
            coin: plan.coin,
            status: "cancelled".to_string(),
            order_id: plan.order_id,
            cloid: plan.cloid,
        }],
    });
    Ok(())
}

/// Cancel all open orders, optionally filtered by coin.
pub async fn cancel_all(
    context: OrderExecutionContext<'_>,
    user: Address,
    args: &CancelAllArgs,
    vault_address: Option<Address>,
) -> Result<(), anyhow::Error> {
    context
        .submission
        .signer
        .ensure_can_attempt_live_signing()?;
    let plan = prepare_cancel_all_orders_plan(context.client, context.resolver, user, args).await?;

    if plan.cancelled_orders != 0 {
        let target = ConsentTarget::new(context.submission, vault_address, None);
        if !args.yes && !confirm_cancel_all(args.coin.as_deref(), plan.cancelled_orders, &target)? {
            return Err(CliError::Configuration("cancel-all aborted".to_string()).into());
        }
    }

    let cancelled_orders = if let Some(action) = plan.action {
        let order_ids: Vec<u64> = match &action {
            Action::Cancel(batch) => batch.cancels.iter().map(|cancel| cancel.oid).collect(),
            _ => Vec::new(),
        };
        let nonce = Utc::now().timestamp_millis() as u64;
        let response = actions::send_l1_action_raw(
            context.submission.api_base_url,
            context.submission.chain,
            context.submission.signer,
            action,
            nonce,
            vault_address,
            "cancel-all failed",
        )
        .await?;
        let statuses = order_statuses_from_response(response, "cancel-all")?;
        if statuses.len() != order_ids.len() {
            return Err(status_count_mismatch(statuses.len(), order_ids.len()).into());
        }
        let rows: Vec<CancelLeg> = order_ids
            .iter()
            .zip(&statuses)
            .map(|(order_id, status)| CancelLeg::from_status(*order_id, status))
            .collect();
        let failures = rejection_errors(rows.iter().map(|row| row.error.as_deref()));
        if !failures.is_empty() {
            return finish_with_failures(
                CancelLegsOutput { rows },
                "Cancelled",
                "cancel all orders",
                failures,
                context.submission.inv,
            );
        }
        statuses.len()
    } else {
        0
    };

    context.submission.inv.print(&CancelAllSummaryOutput {
        rows: vec![CancelAllSummary {
            coin: plan.summary_coin,
            cancelled_orders,
        }],
    });
    Ok(())
}

/// Modify an existing order's price and/or size.
pub async fn modify(
    context: OrderExecutionContext<'_>,
    user: Address,
    args: &ModifyArgs,
    vault_address: Option<Address>,
) -> Result<(), anyhow::Error> {
    context
        .submission
        .signer
        .ensure_can_attempt_live_signing()?;
    let plan = prepare_modify_order_plan(context.client, context.resolver, user, args).await?;
    let nonce = Utc::now().timestamp_millis() as u64;
    let response = actions::send_l1_action_raw(
        context.submission.api_base_url,
        context.submission.chain,
        context.submission.signer,
        plan.action,
        nonce,
        vault_address,
        "modify failed",
    )
    .await?;
    let statuses = order_statuses_from_response(response, "modify")?;
    ensure_action_statuses_ok(&statuses, "modify order")?;
    let mut confirmation = plan.confirmation;
    confirmation.order_id = modification_result_oid(&statuses).or(confirmation.order_id);

    context.submission.inv.print(&ModifyConfirmationOutput {
        rows: vec![confirmation],
    });
    Ok(())
}

/// Create a TWAP order using Hyperliquid's signed TWAP exchange action.
pub async fn twap_create(
    context: OrderSubmissionContext<'_>,
    resolver: &AssetResolver,
    args: &TwapCreateArgs,
    vault_address: Option<Address>,
) -> Result<(), anyhow::Error> {
    context.signer.ensure_can_attempt_live_signing()?;
    let plan = prepare_twap_create_plan(resolver, args)?;
    if context.require_mainnet_confirmation
        && !args.yes
        && !confirm_mainnet_twap(
            &plan.coin,
            args,
            &ConsentTarget::new(context, vault_address, None),
        )?
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
        actions::RawL1ActionMetadata::new(actions::nonce_now()).with_vault_address(vault_address),
        "twap-create rejected",
    )
    .await?;
    let twap_id = parse_twap_create_response(response)?;

    context.inv.print(&TwapCreateOutput {
        rows: vec![TwapCreateConfirmation {
            coin: plan.coin,
            side: plan.side,
            size: plan.size,
            duration_seconds: plan.duration_seconds,
            duration_minutes: plan.duration_minutes,
            status: "running".to_string(),
            twap_id,
        }],
    });
    Ok(())
}

/// Cancel a TWAP order using Hyperliquid's signed TWAP cancel action.
pub async fn twap_cancel(
    context: OrderSubmissionContext<'_>,
    resolver: &AssetResolver,
    args: &TwapCancelArgs,
    vault_address: Option<Address>,
) -> Result<(), anyhow::Error> {
    context.signer.ensure_can_attempt_live_signing()?;
    let plan = prepare_twap_cancel_plan(resolver, args)?;
    let response = actions::send_raw_l1_json_action(
        context.api_base_url,
        context.chain,
        context.signer,
        &plan.action,
        actions::RawL1ActionMetadata::new(actions::nonce_now()).with_vault_address(vault_address),
        "twap-cancel rejected",
    )
    .await?;
    parse_twap_cancel_response(response)?;

    context.inv.print(&TwapCancelOutput {
        rows: vec![TwapCancelConfirmation {
            coin: plan.coin,
            status: "cancelled".to_string(),
            twap_id: plan.twap_id,
        }],
    });
    Ok(())
}

/// Schedule a dead man's switch cancel-all.
pub async fn schedule_cancel(
    context: OrderSubmissionContext<'_>,
    args: &ScheduleCancelArgs,
    vault_address: Option<Address>,
) -> Result<(), anyhow::Error> {
    context.signer.ensure_can_attempt_live_signing()?;
    let mut plan = prepare_schedule_cancel_plan(args, Utc::now())?;
    if context.require_mainnet_confirmation
        && !args.yes
        && !confirm_mainnet_schedule_cancel(
            &plan,
            &ConsentTarget::new(context, vault_address, None),
        )?
    {
        return Err(CliError::Configuration(
            "Mainnet schedule-cancel confirmation required; action cancelled. Rerun with --yes for deliberate automation."
                .to_string(),
        )
        .into());
    }
    if plan.scheduled_at.is_some() {
        plan = prepare_schedule_cancel_plan(args, Utc::now())?;
    }
    let nonce = actions::nonce_now();
    if plan.scheduled_at.is_some() {
        actions::send_l1_action_raw(
            context.api_base_url,
            context.chain,
            context.signer,
            plan.action,
            nonce,
            vault_address,
            "schedule-cancel rejected",
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
            actions::RawL1ActionMetadata::new(nonce).with_vault_address(vault_address),
            "schedule-cancel clear rejected",
        )
        .await?;
    }

    context.inv.print(&ScheduleCancelOutput {
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
    });
    Ok(())
}

fn order_builder_fee(
    args: &CreateArgs,
    asset_kind: TradableAssetKind,
    side: OrderSide,
) -> Result<Option<OrderBuilderFee>, CliError> {
    if asset_kind == TradableAssetKind::Spot
        && side == OrderSide::Buy
        && args.builder.is_none()
        && args.builder_fee_rate.is_none()
    {
        return Ok(None);
    }

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
    if asset_kind == TradableAssetKind::Spot && side == OrderSide::Buy {
        return Err(CliError::Unsupported(
            "spot buy orders do not support builder fees; Hyperliquid builder codes apply to spot sells and both sides of perpetual orders"
                .to_string(),
        ));
    }
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

/// Most problems listed in one batch-file error.
const MAX_LISTED_LEG_PROBLEMS: usize = 5;

fn read_batch_create_orders(path: &Path) -> Result<Vec<BatchCreateOrder>, CliError> {
    let value = read_json_file(path, FilePolicy::json_artifact("orders file"))?;
    let orders_value = value.get("orders").cloned().unwrap_or(value);
    let serde_json::Value::Array(items) = orders_value else {
        return Err(CliError::Configuration(format!(
            "Can't use orders file {}. Expected a JSON array, or an object with an `orders` array.",
            path.display()
        )));
    };
    if items.len() > MAX_BATCH_ORDER_COUNT {
        return Err(CliError::Configuration(format!(
            "orders batch-create supports at most {MAX_BATCH_ORDER_COUNT} order legs, {} given in {}",
            items.len(),
            path.display()
        )));
    }

    let mut orders = Vec::with_capacity(items.len());
    let mut problems = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        let leg_problems = batch_leg_problems(index, &item);
        if leg_problems.is_empty() {
            match serde_json::from_value::<BatchCreateOrder>(item) {
                Ok(order) => orders.push(order),
                Err(err) => problems.push(format!("orders[{index}]: {err}")),
            }
        } else {
            problems.extend(leg_problems);
        }
    }
    if problems.is_empty() {
        return Ok(orders);
    }

    let hidden = problems.len().saturating_sub(MAX_LISTED_LEG_PROBLEMS);
    let mut lines: Vec<String> = problems
        .iter()
        .take(MAX_LISTED_LEG_PROBLEMS)
        .map(|problem| format!("  {problem}"))
        .collect();
    if hidden > 0 {
        lines.push(format!("  and {hidden} more"));
    }
    Err(CliError::Configuration(format!(
        "Can't use orders file {}. Fix these fields:\n{}",
        path.display(),
        lines.join("\n")
    )))
}

/// Every invalid field of one batch leg, named as `orders[<index>].<field>`.
fn batch_leg_problems(index: usize, item: &serde_json::Value) -> Vec<String> {
    let Some(object) = item.as_object() else {
        return vec![format!("orders[{index}] must be an object")];
    };
    let mut problems = Vec::new();
    let mut check =
        |name: &str, required: bool, parse: fn(&serde_json::Value) -> Option<String>| match object
            .get(name)
        {
            None if required => problems.push(format!("orders[{index}].{name} is required")),
            None => {}
            Some(value) => {
                if let Some(reason) = parse(value) {
                    problems.push(format!("orders[{index}].{name}: {reason}"));
                }
            }
        };
    check("coin", true, probe::<String>);
    check("side", true, probe::<OrderSide>);
    check("price", true, probe::<Decimal>);
    check("size", true, probe::<Decimal>);
    check("dex", false, probe::<Option<String>>);
    check("tif", false, probe::<Option<TifArg>>);
    check("reduce_only", false, probe::<bool>);
    check("cloid", false, probe::<Option<String>>);
    problems
}

fn probe<T: serde::de::DeserializeOwned>(value: &serde_json::Value) -> Option<String> {
    serde_json::from_value::<T>(value.clone())
        .err()
        .map(|err| err.to_string())
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
        .map_err(from_sdk)?
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
        return Err(status_count_mismatch(statuses.len(), prepared.legs.len()));
    }
    Ok(prepared
        .legs
        .iter()
        .zip(statuses)
        .map(|(leg, status)| {
            TpslOrderConfirmation::from_tpsl_status(leg, prepared.grouping, status)
        })
        .collect())
}

/// The exchange answered with a different number of statuses than orders sent,
/// so some orders may be live without a matching row.
fn status_count_mismatch(statuses: usize, orders: usize) -> CliError {
    CliError::Internal(anyhow::anyhow!(
        "The exchange returned {statuses} {} for {orders} {}. Check what is live:\n    {}",
        plural(statuses, "status", "statuses"),
        plural(orders, "order", "orders"),
        rendering::CHECK_LIVE_COMMAND
    ))
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
    decode_order_statuses(response)
}

fn parse_tpsl_order_statuses(
    response: serde_json::Value,
) -> Result<Vec<TpslResponseStatus>, CliError> {
    decode_order_statuses(response)
}

fn decode_order_statuses<T: serde::de::DeserializeOwned>(
    response: serde_json::Value,
) -> Result<Vec<T>, CliError> {
    let body = || labelled_untrusted_text(&response.to_string());
    if response.get("type").and_then(serde_json::Value::as_str) != Some("order") {
        return Err(CliError::Internal(anyhow::anyhow!(
            "order placement response had unexpected type: {}",
            body()
        )));
    }
    let statuses = response
        .get("data")
        .and_then(|data| data.get("statuses"))
        .cloned()
        .ok_or_else(|| {
            CliError::Internal(anyhow::anyhow!(
                "order placement response missing statuses: {}",
                body()
            ))
        })?;
    serde_json::from_value(statuses).map_err(|err| {
        CliError::Internal(anyhow::anyhow!(
            "order placement response decode failed: {err}; body={}",
            body()
        ))
    })
}

async fn resolve_tradable_asset(
    client: &HttpClient,
    resolver: &AssetResolver,
    query: &str,
    args: &CreateArgs,
) -> Result<Market, CliError> {
    if let AssetQuery::Outcome(notation) = parse_asset_query(&args.coin) {
        if args.dex.is_some() {
            return Err(CliError::Unsupported(format!(
                "outcome notation '{notation}' cannot be used with --dex; omit --dex for outcome orders"
            )));
        }
        if args.order_type == CreateOrderType::Market {
            return Err(CliError::Unsupported(
                "outcome orders do not support --type market because outcome size decimals are not verified; use an explicit --price and --size"
                    .to_string(),
            ));
        }
        return Ok(Market::Outcome(OutcomeAsset::new(notation)));
    }

    let perp_err = match resolver.resolve_perp_or_fetch(client, query).await {
        Ok(perp) => return Ok(Market::Perp(perp)),
        Err(err) => err,
    };
    if !matches!(parse_asset_query(query), AssetQuery::Hip3 { .. })
        && let Ok(spot) = resolver.resolve_spot(query)
    {
        return Ok(Market::Spot(spot));
    }
    Err(perp_err.into())
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
        return Err(rendering::status_error("create TWAP", error));
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

/// Unique client order ID for internally generated orders (chase quotes,
/// bracket entries): nonce-derived high bits plus a per-run sequence.
fn generated_cloid(seq: u64) -> Result<Cloid, CliError> {
    let raw = format!("0x{:016x}{:016x}", actions::nonce_now(), seq);
    parse_cloid(&raw)
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
        return Err(rendering::status_error("cancel TWAP", error));
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
        .map_err(from_sdk)?
        .ok_or_else(|| CliError::Unsupported(format!("unknown order {}", identifier.display())))?;
    Ok(order.order)
}

async fn asset_index_for_order(
    client: &HttpClient,
    resolver: &AssetResolver,
    order: &BasicOrder,
) -> Result<usize, CliError> {
    if let Some(internal_id) = parse_spot_coin_key(&order.coin) {
        return Ok(resolver
            .spot_asset_index_for_internal_id(internal_id)
            .unwrap_or(internal_id));
    }

    resolver
        .resolve_or_fetch(client, &order.coin)
        .await?
        .asset_index()
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
    if let Some(internal_id) = parse_spot_coin_key(coin) {
        return Ok(Some(spot_internal_coin_key(internal_id)));
    }

    let query = match parse_asset_query(coin) {
        AssetQuery::Outcome(_) => coin.to_string(),
        _ => qualify_dex_asset(dex, coin),
    };
    match resolver.resolve(&query) {
        Ok(Market::Perp(perp)) => Ok(Some(perp.display_name())),
        Ok(Market::Spot(spot)) => Ok(Some(spot_coin_key(spot.index))),
        Ok(Market::Outcome(outcome)) => Ok(Some(outcome_coin(outcome.parse()?.encoding))),
        Err(err) => match parse_asset_query(&query) {
            AssetQuery::Hip3 { dex, token } => Ok(Some(format!("{dex}:{token}"))),
            _ => Err(err.into()),
        },
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

/// Fail on the first rejected status, with the exchange text sanitized and
/// labelled. `action` completes "Couldn't ...", such as `cancel order`.
fn ensure_action_statuses_ok(
    statuses: &[OrderResponseStatus],
    action: &str,
) -> Result<usize, CliError> {
    for status in statuses {
        if let OrderResponseStatus::Error(err) = status {
            return Err(rendering::status_error(action, err));
        }
    }
    Ok(statuses.len())
}

fn modification_result_oid(statuses: &[OrderResponseStatus]) -> Option<u64> {
    statuses.iter().find_map(|status| match status {
        OrderResponseStatus::Resting { oid, .. } | OrderResponseStatus::Filled { oid, .. } => {
            Some(*oid)
        }
        OrderResponseStatus::Success
        | OrderResponseStatus::WaitingForTrigger
        | OrderResponseStatus::WaitingForFill
        | OrderResponseStatus::Error(_) => None,
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
        OrderType::TrailingStopMarket => Err(CliError::Unsupported(
            "orders modify can't change a Trailing Stop Market order. Cancel it with `hyperliquid orders cancel <OID>`, then place a new one with `hyperliquid orders trailing-stop`."
                .to_string(),
        )),
        OrderType::TwapSlice => Err(CliError::Unsupported(
            "orders modify can't change a Twap Slice order. Cancel the TWAP with `hyperliquid orders twap-cancel`, then re-create it with `hyperliquid orders twap-create`."
                .to_string(),
        )),
        OrderType::VaultClose => Err(CliError::Unsupported(
            "orders modify can't change a Vault Close order. The exchange places it when a vault closes positions; cancel it with `hyperliquid orders cancel <OID>` and place a regular order instead."
                .to_string(),
        )),
        OrderType::SpotDustConversion => Err(CliError::Unsupported(
            "orders modify can't change a Spot Dust Conversion order. Cancel it with `hyperliquid orders cancel <OID>` and place a regular order instead."
                .to_string(),
        )),
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

fn confirm_cancel_all(
    coin: Option<&str>,
    count: usize,
    target: &ConsentTarget<'_>,
) -> Result<bool, CliError> {
    let scope = coin.map(|coin| format!("{coin} ")).unwrap_or_default();
    let question = format!(
        "Cancel {count} open {scope}{} on {}?",
        plural(count, "order", "orders"),
        target.network()
    );
    ask_confirmation(target, &[], &question)
}

fn confirm_mainnet_schedule_cancel(
    plan: &ScheduleCancelPlan,
    target: &ConsentTarget<'_>,
) -> Result<bool, CliError> {
    let question = if let Some(scheduled_at) = plan.scheduled_at.as_ref() {
        format!(
            "Schedule cancel-all for {} on {}?",
            scheduled_at.to_rfc3339(),
            target.network()
        )
    } else {
        format!("Clear scheduled cancel-all on {}?", target.network())
    };
    ask_confirmation(target, &[], &question)
}

async fn market_mid_price(client: &HttpClient, lookup: &MidLookup) -> Result<Decimal, CliError> {
    let mids = client
        .all_mids(lookup.dex.clone())
        .await
        .map_err(from_sdk)?;
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

fn confirm_mainnet_order(
    prepared: &PreparedOrder,
    target: &ConsentTarget<'_>,
) -> Result<bool, CliError> {
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
    let mut details = vec![format!(
        "Order: {} {} order for {} {} at price {}{}{}, {}",
        prepared.side,
        prepared.order_type,
        prepared.size,
        prepared.coin,
        prepared.price,
        trigger,
        amount,
        reduce_only
    )];
    if let Some(slippage_warning) = &prepared.warning {
        details.push(prompt::warning(slippage_warning));
    }
    ask_confirmation(
        target,
        &details,
        &format!("Place this order on {}?", target.network()),
    )
}

fn confirm_mainnet_order_batch(
    label: &str,
    prepared: &PreparedOrderBatch,
    target: &ConsentTarget<'_>,
) -> Result<bool, CliError> {
    let count = prepared.legs.len();
    let orders = plural(count, "order", "orders");
    ask_confirmation(
        target,
        &[format!("Orders: {count} {label} {orders}")],
        &format!("Submit {count} {label} {orders} on {}?", target.network()),
    )
}

fn confirm_mainnet_tpsl_batch(
    prepared: &PreparedTpslBatch,
    target: &ConsentTarget<'_>,
) -> Result<bool, CliError> {
    let parent = prepared
        .legs
        .iter()
        .find(|leg| leg.leg == TpslLegKind::Parent);
    let summary = if let Some(parent) = parent {
        let children = prepared.legs.len().saturating_sub(1);
        format!(
            "Orders: {} parent order for {} {} at price {} with {children} grouped child {} using {}",
            parent.side,
            parent.size,
            parent.coin,
            parent.price,
            plural(children, "order", "orders"),
            prepared.grouping
        )
    } else {
        let first = prepared.legs.first().ok_or_else(|| {
            CliError::Configuration("TP/SL batch requires at least one leg".to_string())
        })?;
        format!(
            "Orders: {} grouped TP/SL {} for {} using {}",
            prepared.legs.len(),
            plural(prepared.legs.len(), "order", "orders"),
            first.coin,
            prepared.grouping
        )
    };
    let mut details = vec![summary];
    for leg in &prepared.legs {
        if let Some(warning) = &leg.warning {
            details.push(prompt::warning(warning));
        }
    }
    ask_confirmation(
        target,
        &details,
        &format!("Place this TP/SL batch on {}?", target.network()),
    )
}

fn confirm_mainnet_twap(
    coin: &str,
    args: &TwapCreateArgs,
    target: &ConsentTarget<'_>,
) -> Result<bool, CliError> {
    let details = [format!(
        "TWAP: {} {} {} over {} seconds",
        args.side, args.size, coin, args.duration
    )];
    ask_confirmation(
        target,
        &details,
        &format!("Create this TWAP on {}?", target.network()),
    )
}

/// Target facts every consent preview shows before the question.
struct ConsentTarget<'a> {
    mode: &'a Mode,
    chain: Chain,
    signer: &'a SelectedSigner,
    vault_address: Option<Address>,
    builder: Option<&'a OrderBuilderFee>,
}

impl<'a> ConsentTarget<'a> {
    fn new(
        submission: OrderSubmissionContext<'a>,
        vault_address: Option<Address>,
        builder: Option<&'a OrderBuilderFee>,
    ) -> Self {
        Self {
            mode: submission.inv.mode(),
            chain: submission.chain,
            signer: submission.signer,
            vault_address,
            builder,
        }
    }

    fn network(&self) -> &'static str {
        match self.chain {
            Chain::Mainnet => "mainnet",
            Chain::Testnet => "testnet",
        }
    }

    fn preview_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("Network: {}", self.network()),
            format!("Signer: {}", self.signer.address()),
        ];
        if let Some(vault) = self.vault_address {
            lines.push(format!("Acting as: {vault} (vault or subaccount)"));
        }
        if let Some(builder) = self.builder {
            lines.push(format!(
                "Builder: {} (max fee {}%)",
                builder.b,
                Decimal::new(i64::try_from(builder.f).unwrap_or(i64::MAX), 3).normalize()
            ));
        }
        lines
    }
}

/// Print the consent preview block on stderr, then ask `question`.
///
/// Runs that can't prompt fail with `--yes` as the way to answer before the
/// block is printed.
fn ask_confirmation(
    target: &ConsentTarget<'_>,
    details: &[String],
    question: &str,
) -> Result<bool, CliError> {
    prompt::ensure_can_prompt(target.mode, "--yes")?;
    let mut stderr = io::stderr();
    for line in target.preview_lines().iter().chain(details) {
        writeln!(stderr, "{line}").map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    }
    prompt::confirm_warning(target.mode, question, "--yes")
}

/// Pick the singular or plural noun for `count`.
pub(crate) fn plural<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_support::hip3_perp;
    use crate::assets::{AssetMetadata, PerpAsset, SpotAsset};

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
                hip3_perp("testdex", "HYPE", 11_100),
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
            take_profit: Some(TriggerPriceSpec::Absolute(Decimal::from(55_000))),
            stop_loss: None,
            grouping: PositionTpslGroupingArg::PositionTpsl,
            side: Some(OrderSide::Sell),
            size: Some(Decimal::new(1, 1)),
            on_behalf_of: None,
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
            on_behalf_of: None,
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
    async fn spot_buy_builder_fees_are_rejected() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.coin = "HYPE/USDC".to_string();
        args.builder = Some("0x1111111111111111111111111111111111111111".to_string());
        args.builder_fee_rate = Some("0.01%".to_string());

        let err = prepare_order(&client, &resolver(), &args)
            .await
            .unwrap_err();

        assert_eq!(err.exit_code(), 13);
        assert!(err.to_string().contains("spot buy orders"));
        assert!(err.to_string().contains("spot sells"));
    }

    #[tokio::test]
    async fn spot_sell_builder_fees_are_allowed() {
        let client = HttpClient::new(hypersdk::hypercore::Chain::Testnet);
        let mut args = base_args();
        args.coin = "HYPE/USDC".to_string();
        args.side = OrderSide::Sell;
        args.builder = Some("0x1111111111111111111111111111111111111111".to_string());
        args.builder_fee_rate = Some("0.01%".to_string());

        let prepared = prepare_order(&client, &resolver(), &args).await.unwrap();

        let builder = prepared.builder.unwrap();
        assert_eq!(builder.b, "0x1111111111111111111111111111111111111111");
        assert_eq!(builder.f, 10);
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
        let asset = Market::Spot(SpotAsset {
            symbol: "HYPE/USDC".to_string(),
            index: 11_035,
            base: "HYPE".to_string(),
            quote: "USDC".to_string(),
            base_sz_decimals: 2,
        });

        let err = validate_create_resolved_asset(&args, &asset).unwrap_err();

        assert_eq!(err.exit_code(), 13);
        assert!(
            err.to_string()
                .contains("trigger orders currently support perpetual markets only")
        );
    }

    #[test]
    fn spot_buy_builder_fees_are_rejected_after_asset_resolution() {
        let mut args = base_args();
        args.coin = "HYPE/USDC".to_string();
        args.builder = Some("0x1111111111111111111111111111111111111111".to_string());
        args.builder_fee_rate = Some("0.01%".to_string());
        let asset = Market::Spot(SpotAsset {
            symbol: "HYPE/USDC".to_string(),
            index: 11_035,
            base: "HYPE".to_string(),
            quote: "USDC".to_string(),
            base_sz_decimals: 2,
        });

        let err = validate_create_resolved_asset(&args, &asset).unwrap_err();

        assert_eq!(err.exit_code(), 13);
        assert!(err.to_string().contains("spot buy orders"));
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

    #[test]
    fn order_response_parser_accepts_waiting_statuses_without_oid() {
        let response = serde_json::json!({
            "type": "order",
            "data": { "statuses": ["waitingForTrigger", "waitingForFill"] }
        });

        let statuses = parse_order_statuses(response).unwrap();

        assert!(matches!(
            statuses.as_slice(),
            [
                OrderResponseStatus::WaitingForTrigger,
                OrderResponseStatus::WaitingForFill
            ]
        ));
        assert_eq!(
            ensure_action_statuses_ok(&statuses, "place order").unwrap(),
            2
        );
        assert_eq!(modification_result_oid(&statuses), None);
    }

    fn basic_order(order_type: OrderType) -> BasicOrder {
        BasicOrder {
            timestamp: 0,
            coin: "BTC".to_string(),
            side: hypersdk::hypercore::Side::Bid,
            limit_px: Decimal::from(50_000),
            sz: Decimal::ONE,
            oid: 7,
            orig_sz: Decimal::ONE,
            cloid: None,
            order_type,
            tif: None,
            reduce_only: false,
            is_trigger: None,
            trigger_px: None,
            trigger_condition: None,
            is_position_tpsl: None,
        }
    }

    #[test]
    fn modify_rejects_order_types_that_cannot_be_replaced() {
        for (order_type, name, hint) in [
            (
                OrderType::TrailingStopMarket,
                "Trailing Stop Market",
                "hyperliquid orders cancel <OID>",
            ),
            (OrderType::TwapSlice, "Twap Slice", "orders twap-cancel"),
            (OrderType::VaultClose, "Vault Close", "orders cancel <OID>"),
            (
                OrderType::SpotDustConversion,
                "Spot Dust Conversion",
                "orders cancel <OID>",
            ),
        ] {
            let err = order_type_placement(&basic_order(order_type), None, None).unwrap_err();

            assert!(matches!(err, CliError::Unsupported(_)));
            assert_eq!(err.exit_code(), 13);
            let message = err.to_string();
            assert!(message.contains(name), "{message}");
            assert!(message.contains(hint), "{message}");
        }
    }
}
