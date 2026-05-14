use super::validation::{
    reject_spot_reduce_only, reject_spot_tpsl_grouping, reject_spot_trigger_order, require_decimal,
};
use super::*;

pub struct OrderDryRunPlan {
    command: &'static str,
    would_execute: &'static str,
    args: serde_json::Value,
}

impl OrderDryRunPlan {
    fn new(command: &'static str, would_execute: &'static str, args: serde_json::Value) -> Self {
        Self {
            command,
            would_execute,
            args,
        }
    }

    #[must_use]
    pub fn command(&self) -> &'static str {
        self.command
    }

    #[must_use]
    pub fn would_execute(&self) -> &'static str {
        self.would_execute
    }

    #[must_use]
    pub fn into_args(self) -> serde_json::Value {
        self.args
    }
}

pub(crate) struct CancelOrderPlan {
    pub(crate) action: Action,
    pub(crate) coin: String,
    pub(crate) order_id: Option<u64>,
    pub(crate) cloid: Option<String>,
}

pub(crate) struct CancelAllOrdersPlan {
    pub(crate) action: Option<Action>,
    pub(crate) cancelled_orders: usize,
    pub(crate) summary_coin: String,
}

pub(crate) struct ModifyOrderPlan {
    pub(crate) action: Action,
    pub(crate) confirmation: ModifyConfirmation,
}

pub(crate) enum CreateOrderSubmission {
    Single(PreparedOrder),
    NormalTpsl(PreparedTpslBatch),
}

pub(crate) struct CreateOrderPlan {
    pub(crate) submission: CreateOrderSubmission,
    dry_run_args: serde_json::Value,
}

impl CreateOrderPlan {
    fn into_dry_run_args(self) -> serde_json::Value {
        self.dry_run_args
    }
}

pub(crate) struct BatchCreateOrderPlan {
    pub(crate) prepared: PreparedOrderBatch,
    dry_run_args: serde_json::Value,
}

impl BatchCreateOrderPlan {
    fn into_dry_run_args(self) -> serde_json::Value {
        self.dry_run_args
    }
}

pub(crate) struct TwapCreatePlan {
    pub(crate) action: TwapExchangeAction,
    pub(crate) coin: String,
    pub(crate) asset: usize,
    pub(crate) side: String,
    pub(crate) size: Decimal,
    pub(crate) duration_seconds: u64,
    pub(crate) duration_minutes: u64,
}

impl TwapCreatePlan {
    fn dry_run_args(&self, raw_coin: &str, dex: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "coin": raw_coin,
            "dex": dex,
            "side": self.side,
            "size": self.size.to_string(),
            "duration": self.duration_seconds,
            "duration_minutes": self.duration_minutes,
            "asset_id": self.asset,
            "resolved_asset": self.coin,
        })
    }
}

fn insert_margin_mode(preview: &mut serde_json::Value, margin_mode: MarginModeArg) {
    if let Some(object) = preview.as_object_mut() {
        object.insert(
            "margin_mode".to_string(),
            serde_json::json!(margin_mode.to_string()),
        );
    }
}

pub(crate) struct TwapCancelPlan {
    pub(crate) action: TwapExchangeAction,
    pub(crate) coin: String,
    pub(crate) twap_id: u64,
}

impl TwapCancelPlan {
    fn dry_run_args(&self, raw_coin: &str, dex: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "twap_id": self.twap_id,
            "coin": raw_coin,
            "dex": dex,
            "resolved_asset": self.coin,
        })
    }
}

pub(crate) struct ScheduleCancelPlan {
    pub(crate) action: Action,
    pub(crate) scheduled_at: Option<chrono::DateTime<Utc>>,
    pub(crate) in_seconds: Option<u64>,
}

impl ScheduleCancelPlan {
    fn dry_run_args(&self) -> serde_json::Value {
        match (&self.scheduled_at, self.in_seconds) {
            (Some(scheduled_at), Some(in_seconds)) => serde_json::json!({
                "mode": "set",
                "in_duration_ms": in_seconds * 1000,
                "scheduled_time": scheduled_at.to_rfc3339(),
                "scheduled_time_ms": scheduled_at.timestamp_millis() as u64,
            }),
            _ => serde_json::json!({
                "mode": "clear",
            }),
        }
    }
}

pub(crate) async fn prepare_order(
    client: &HttpClient,
    resolver: &AssetResolver,
    args: &CreateArgs,
) -> Result<PreparedOrder, CliError> {
    validate_create_args(args)?;

    let query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let asset = resolve_tradable_asset(client, resolver, &query, args).await?;
    let (coin, index, sz_decimals, mid_lookup, price_tick, asset_kind, amount_unit) = match asset {
        TradableAsset::Perp {
            name,
            index,
            dex,
            sz_decimals,
            collateral,
        } => {
            let coin = dex
                .as_ref()
                .map(|dex| format!("{dex}:{name}"))
                .unwrap_or_else(|| name.clone());
            (
                coin.clone(),
                index,
                sz_decimals,
                MidLookup {
                    coin,
                    dex,
                    original_coin: query.clone(),
                },
                PriceTick::for_perp(i64::from(sz_decimals)),
                TradableAssetKind::Perp,
                collateral,
            )
        }
        TradableAsset::Spot {
            symbol,
            index,
            base_sz_decimals,
            quote,
        } => (
            symbol,
            index,
            base_sz_decimals,
            MidLookup {
                coin: spot_mid_key(index),
                dex: None,
                original_coin: args.coin.clone(),
            },
            PriceTick::for_spot(i64::from(base_sz_decimals)),
            TradableAssetKind::Spot,
            quote,
        ),
        TradableAsset::Outcome {
            notation, asset_id, ..
        } => (
            notation,
            asset_id,
            0,
            MidLookup {
                coin: args.coin.clone(),
                dex: None,
                original_coin: args.coin.clone(),
            },
            PriceTick::for_spot(0),
            TradableAssetKind::Outcome,
            "USDH".to_string(),
        ),
    };

    if !matches!(asset_kind, TradableAssetKind::Perp) && args.margin_mode.is_some() {
        return Err(CliError::Configuration(
            "orders create --margin-mode is only supported for perpetual orders".to_string(),
        ));
    }

    if matches!(
        asset_kind,
        TradableAssetKind::Spot | TradableAssetKind::Outcome
    ) {
        reject_spot_reduce_only(args)?;
        reject_spot_trigger_order(args)?;
        reject_spot_tpsl_grouping(args)?;
    }

    let (price, trigger_price, size, amount, warning, order_type, tif) = match args.order_type {
        CreateOrderType::Limit => {
            let price = require_decimal(args.price, "--price", "limit orders")?;
            let size = require_decimal(args.size, "--size", "limit orders")?;
            (
                price,
                None,
                size,
                None,
                None,
                OrderTypePlacement::Limit {
                    tif: args.tif.into(),
                },
                Some(args.tif),
            )
        }
        CreateOrderType::StopLoss | CreateOrderType::TakeProfit => {
            let trigger_price = market_trigger_price(args)?;
            let size = require_decimal(args.size, "--size", "market trigger orders")?;
            let tpsl = args
                .order_type
                .tpsl()
                .expect("market trigger order type must map to TP/SL");
            (
                trigger_price,
                Some(trigger_price),
                size,
                None,
                None,
                OrderTypePlacement::Trigger {
                    is_market: true,
                    trigger_px: trigger_price,
                    tpsl,
                },
                None,
            )
        }
        CreateOrderType::StopLimit | CreateOrderType::TakeLimit => {
            let price = require_decimal(args.price, "--price", "limit trigger orders")?;
            let trigger_price = require_decimal(
                args.trigger_price,
                "--trigger-price",
                "limit trigger orders",
            )?;
            let size = require_decimal(args.size, "--size", "limit trigger orders")?;
            let tpsl = args
                .order_type
                .tpsl()
                .expect("limit trigger order type must map to TP/SL");
            (
                price,
                Some(trigger_price),
                size,
                None,
                None,
                OrderTypePlacement::Trigger {
                    is_market: false,
                    trigger_px: trigger_price,
                    tpsl,
                },
                None,
            )
        }
        CreateOrderType::Market => {
            let amount = require_decimal(args.amount, "--amount", "market orders")?;
            let mid = market_mid_price(client, &mid_lookup).await?;
            let price = market_limit_price(mid, args.side, args.max_slippage_bps, price_tick)?;
            let size = (amount / mid).round_dp(sz_decimals);
            if size <= Decimal::ZERO {
                return Err(CliError::Configuration(format!(
                    "amount is too small for {coin}; derived size rounds to zero"
                )));
            }
            (
                price,
                None,
                size,
                Some(amount),
                Some(format!(
                    "Slippage warning: market order uses a {} bps protective price around mid {}",
                    args.max_slippage_bps, mid
                )),
                OrderTypePlacement::Limit {
                    tif: TimeInForce::FrontendMarket,
                },
                None,
            )
        }
    };

    Ok(PreparedOrder {
        request: OrderRequest {
            asset: index,
            is_buy: args.side.is_buy(),
            limit_px: price,
            sz: size,
            reduce_only: args.reduce_only || args.order_type.uses_implicit_reduce_only(),
            order_type,
            cloid: args
                .cloid
                .as_deref()
                .map(parse_cloid)
                .transpose()?
                .unwrap_or_default(),
        },
        asset_kind,
        coin,
        side: args.side,
        order_type: args.order_type,
        tif,
        price,
        trigger_price,
        size,
        amount,
        amount_unit,
        warning,
        builder: order_builder_fee(args, asset_kind)?,
    })
}

pub(crate) async fn prepare_scale_batch(
    client: &HttpClient,
    resolver: &AssetResolver,
    args: &ScaleArgs,
) -> Result<PreparedOrderBatch, CliError> {
    validate_scale_args(args)?;
    let prices = scale_prices(args.start_price, args.end_price, args.order_count)?;
    let sizes = scale_sizes(args.total_size, args.order_count)?;
    let legs = prices
        .into_iter()
        .zip(sizes)
        .enumerate()
        .map(|(index, (price, size))| {
            (
                index,
                CreateArgs {
                    coin: args.coin.clone(),
                    dex: args.dex.clone(),
                    side: args.side,
                    price: Some(price),
                    trigger_price: None,
                    size: Some(size),
                    amount: None,
                    order_type: CreateOrderType::Limit,
                    tif: args.tif,
                    reduce_only: args.reduce_only,
                    max_slippage_bps: DEFAULT_MARKET_ORDER_SLIPPAGE_BPS,
                    take_profit: None,
                    stop_loss: None,
                    grouping: None,
                    on_behalf_of: args.on_behalf_of.clone(),
                    margin_mode: args.margin_mode,
                    builder: None,
                    builder_fee_rate: None,
                    cloid: None,
                    yes: args.yes,
                },
            )
        })
        .collect::<Vec<_>>();
    prepare_order_batch_from_create_args(client, resolver, legs).await
}

pub(crate) async fn prepare_batch_create_orders(
    client: &HttpClient,
    resolver: &AssetResolver,
    args: &BatchCreateArgs,
    orders: Vec<BatchCreateOrder>,
) -> Result<PreparedOrderBatch, CliError> {
    validate_batch_create_args(args)?;
    let legs = orders
        .into_iter()
        .enumerate()
        .map(|(index, order)| {
            (
                index,
                CreateArgs {
                    coin: order.coin,
                    dex: order.dex,
                    side: order.side,
                    price: Some(order.price),
                    trigger_price: None,
                    size: Some(order.size),
                    amount: None,
                    order_type: CreateOrderType::Limit,
                    tif: order.tif.unwrap_or(TifArg::Gtc),
                    reduce_only: order.reduce_only,
                    max_slippage_bps: DEFAULT_MARKET_ORDER_SLIPPAGE_BPS,
                    take_profit: None,
                    stop_loss: None,
                    grouping: None,
                    on_behalf_of: args.on_behalf_of.clone(),
                    margin_mode: None,
                    builder: None,
                    builder_fee_rate: None,
                    cloid: order.cloid,
                    yes: args.yes,
                },
            )
        })
        .collect::<Vec<_>>();
    prepare_order_batch_from_create_args(client, resolver, legs).await
}

async fn prepare_order_batch_from_create_args(
    client: &HttpClient,
    resolver: &AssetResolver,
    legs: Vec<(usize, CreateArgs)>,
) -> Result<PreparedOrderBatch, CliError> {
    let mut prepared_legs = Vec::with_capacity(legs.len());
    for (index, args) in legs {
        let order = prepare_order(client, resolver, &args).await?;
        prepared_legs.push(PreparedOrderLeg {
            leg_index: index,
            order,
        });
    }
    let batch = BatchOrder {
        orders: prepared_legs
            .iter()
            .map(|leg| leg.order.request.clone())
            .collect(),
        grouping: OrderGrouping::Na,
    };
    Ok(PreparedOrderBatch {
        batch,
        legs: prepared_legs,
    })
}

pub(crate) fn create_has_tpsl_legs(args: &CreateArgs) -> bool {
    args.take_profit.is_some() || args.stop_loss.is_some()
}

pub(crate) fn prepare_normal_tpsl_batch(
    parent: PreparedOrder,
    args: &CreateArgs,
) -> Result<PreparedTpslBatch, CliError> {
    if parent.asset_kind == TradableAssetKind::Spot {
        return Err(CliError::Unsupported(
            "orders create TP/SL grouping currently supports perpetual markets only".to_string(),
        ));
    }
    let grouping = TpslGroupingArg::NormalTpsl;
    let child_side = parent.side.opposite();
    validate_tpsl_price_ordering(
        child_side,
        args.take_profit,
        args.stop_loss,
        "orders create",
    )?;
    let mut legs = vec![PreparedTpslLeg {
        request: parent.request.clone(),
        leg: TpslLegKind::Parent,
        coin: parent.coin.clone(),
        side: parent.side,
        order_type: parent.order_type,
        price: parent.price,
        size: parent.size,
        tif: parent.tif,
        reduce_only: parent.request.reduce_only,
        warning: parent.warning.clone(),
    }];

    push_tpsl_child_leg(
        &mut legs,
        TpslLegKind::TakeProfit,
        &parent,
        child_side,
        args.take_profit,
        &mut None,
    );
    push_tpsl_child_leg(
        &mut legs,
        TpslLegKind::StopLoss,
        &parent,
        child_side,
        args.stop_loss,
        &mut None,
    );

    Ok(batch_from_legs(grouping, legs))
}

pub(crate) async fn prepare_position_tpsl_batch(
    client: &HttpClient,
    resolver: &AssetResolver,
    signer: &SelectedSigner,
    args: &TpslArgs,
) -> Result<PreparedTpslBatch, CliError> {
    validate_tpsl_args(args)?;
    let resolved = resolve_tpsl_perp(resolver, args.dex.as_deref(), &args.coin)?;
    let (side, size) = match (args.side, args.size) {
        (Some(side), Some(size)) => (side, size),
        (None, None) => {
            let position = lookup_position_for_tpsl(
                client,
                signer.query_address(),
                resolved.dex.clone(),
                &resolved.name,
            )
            .await?;
            position_close_side_and_size(&position)?
        }
        _ => unreachable!("validate_tpsl_args rejects partial side/size"),
    };
    validate_tpsl_price_ordering(side, args.take_profit, args.stop_loss, "orders tpsl")?;

    let parsed_cloid = args.cloid.as_deref().map(parse_cloid).transpose()?;

    Ok(build_position_tpsl_batch(
        TpslGroupingArg::PositionTpsl,
        resolved.coin,
        resolved.index,
        resolved.collateral,
        side,
        size,
        args.take_profit,
        args.stop_loss,
        parsed_cloid,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_position_tpsl_batch(
    grouping: TpslGroupingArg,
    coin: String,
    asset: usize,
    amount_unit: String,
    side: OrderSide,
    size: Decimal,
    take_profit: Option<Decimal>,
    stop_loss: Option<Decimal>,
    cloid: Option<Cloid>,
) -> PreparedTpslBatch {
    let resolved_cloid = cloid.unwrap_or_default();
    let parent = PreparedOrder {
        request: OrderRequest {
            asset,
            is_buy: side.is_buy(),
            limit_px: Decimal::ZERO,
            sz: size,
            reduce_only: true,
            order_type: OrderTypePlacement::Limit {
                tif: TimeInForce::Ioc,
            },
            cloid: resolved_cloid,
        },
        asset_kind: TradableAssetKind::Perp,
        coin,
        side,
        order_type: CreateOrderType::Limit,
        tif: None,
        price: Decimal::ZERO,
        trigger_price: None,
        size,
        amount: None,
        amount_unit,
        warning: None,
        builder: None,
    };
    let mut legs = Vec::new();
    let mut remaining_cloid = cloid;
    push_tpsl_child_leg(
        &mut legs,
        TpslLegKind::TakeProfit,
        &parent,
        side,
        take_profit,
        &mut remaining_cloid,
    );
    push_tpsl_child_leg(
        &mut legs,
        TpslLegKind::StopLoss,
        &parent,
        side,
        stop_loss,
        &mut remaining_cloid,
    );
    batch_from_legs(grouping, legs)
}

fn push_tpsl_child_leg(
    legs: &mut Vec<PreparedTpslLeg>,
    leg: TpslLegKind,
    parent: &PreparedOrder,
    side: OrderSide,
    price: Option<Decimal>,
    cloid: &mut Option<Cloid>,
) {
    let Some(price) = price else {
        return;
    };
    let leg_cloid = cloid.take().unwrap_or_default();
    let tpsl = match leg {
        TpslLegKind::TakeProfit => TpSl::Tp,
        TpslLegKind::StopLoss => TpSl::Sl,
        TpslLegKind::Parent => return,
    };
    let order_type = match leg {
        TpslLegKind::TakeProfit => CreateOrderType::TakeProfit,
        TpslLegKind::StopLoss => CreateOrderType::StopLoss,
        TpslLegKind::Parent => parent.order_type,
    };
    legs.push(PreparedTpslLeg {
        request: OrderRequest {
            asset: parent.request.asset,
            is_buy: side.is_buy(),
            limit_px: price,
            sz: parent.request.sz,
            reduce_only: true,
            order_type: OrderTypePlacement::Trigger {
                is_market: true,
                trigger_px: price,
                tpsl,
            },
            cloid: leg_cloid,
        },
        leg,
        coin: parent.coin.clone(),
        side,
        order_type,
        price,
        size: parent.request.sz,
        tif: None,
        reduce_only: true,
        warning: None,
    });
}

fn batch_from_legs(grouping: TpslGroupingArg, legs: Vec<PreparedTpslLeg>) -> PreparedTpslBatch {
    let batch = BatchOrder {
        orders: legs.iter().map(|leg| leg.request.clone()).collect(),
        grouping: grouping.to_sdk(),
    };
    PreparedTpslBatch {
        batch,
        grouping,
        legs,
    }
}

pub(crate) fn validate_tpsl_price_ordering(
    close_side: OrderSide,
    take_profit: Option<Decimal>,
    stop_loss: Option<Decimal>,
    command: &'static str,
) -> Result<(), CliError> {
    let (Some(take_profit), Some(stop_loss)) = (take_profit, stop_loss) else {
        return Ok(());
    };
    let valid = match close_side {
        OrderSide::Sell => take_profit > stop_loss,
        OrderSide::Buy => take_profit < stop_loss,
    };
    if valid {
        Ok(())
    } else {
        let hint = match close_side {
            OrderSide::Sell => {
                "closing a long position requires --take-profit greater than --stop-loss"
            }
            OrderSide::Buy => {
                "closing a short position requires --take-profit less than --stop-loss"
            }
        };
        Err(CliError::Configuration(format!("{command}: {hint}")))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedTpslPerp {
    pub(crate) coin: String,
    pub(crate) name: String,
    pub(crate) index: usize,
    pub(crate) dex: Option<String>,
    pub(crate) collateral: String,
}

pub(crate) fn resolve_tpsl_perp(
    resolver: &AssetResolver,
    dex: Option<&str>,
    coin: &str,
) -> Result<ResolvedTpslPerp, CliError> {
    let query = qualify_dex_asset(dex, coin);
    match resolver.resolve_perp(&query)? {
        ResolvedAsset::Perp {
            name,
            index,
            dex,
            collateral,
            ..
        } => {
            let display = dex
                .as_ref()
                .map(|dex| format!("{dex}:{name}"))
                .unwrap_or_else(|| name.clone());
            Ok(ResolvedTpslPerp {
                coin: display,
                name,
                index,
                dex,
                collateral,
            })
        }
        _ => Err(CliError::Unsupported(
            "TP/SL grouping currently supports perpetual markets only".to_string(),
        )),
    }
}

/// Build the dry-run argument object from the same prepared order used for submission.
pub async fn create_dry_run_args(
    client: &HttpClient,
    resolver: &AssetResolver,
    args: &CreateArgs,
) -> Result<serde_json::Value, CliError> {
    Ok(prepare_create_order_plan(client, resolver, args)
        .await?
        .into_dry_run_args())
}

/// Build `orders create` dry-run argument details, including grouped child payloads.
pub async fn create_dry_run_preview(
    client: &HttpClient,
    resolver: &AssetResolver,
    args: &CreateArgs,
) -> Result<serde_json::Value, CliError> {
    create_dry_run_args(client, resolver, args).await
}

pub async fn create_dry_run_plan(
    client: &HttpClient,
    resolver: &AssetResolver,
    args: &CreateArgs,
) -> Result<OrderDryRunPlan, CliError> {
    let mut preview = create_dry_run_preview(client, resolver, args).await?;
    if let Some(args_object) = preview.as_object_mut() {
        args_object.insert(
            "on_behalf_of".to_string(),
            serde_json::json!(args.on_behalf_of),
        );
    }
    Ok(OrderDryRunPlan::new(
        "orders create",
        "submit_order",
        preview,
    ))
}

/// Build the dry-run argument object for generated scale orders.
pub async fn scale_dry_run_args(
    client: &HttpClient,
    resolver: &AssetResolver,
    args: &ScaleArgs,
) -> Result<serde_json::Value, CliError> {
    let prepared = prepare_scale_batch(client, resolver, args).await?;
    Ok(order_batch_dry_run_args(&prepared))
}

pub async fn scale_dry_run_plan(
    client: &HttpClient,
    resolver: &AssetResolver,
    args: &ScaleArgs,
) -> Result<OrderDryRunPlan, CliError> {
    let mut preview = scale_dry_run_args(client, resolver, args).await?;
    insert_margin_mode(
        &mut preview,
        args.margin_mode.unwrap_or(MarginModeArg::Cross),
    );
    if let Some(args_object) = preview.as_object_mut() {
        args_object.insert(
            "on_behalf_of".to_string(),
            serde_json::json!(args.on_behalf_of),
        );
    }
    Ok(OrderDryRunPlan::new(
        "orders scale",
        "submit_order_batch",
        preview,
    ))
}

/// Build the dry-run argument object for explicit JSON order batches.
pub async fn batch_create_dry_run_args(
    client: &HttpClient,
    resolver: &AssetResolver,
    args: &BatchCreateArgs,
    orders: Vec<BatchCreateOrder>,
) -> Result<serde_json::Value, CliError> {
    Ok(
        prepare_batch_create_order_plan(client, resolver, args, orders)
            .await?
            .into_dry_run_args(),
    )
}

pub async fn batch_create_dry_run_plan(
    client: &HttpClient,
    resolver: &AssetResolver,
    args: &BatchCreateArgs,
    orders: Vec<BatchCreateOrder>,
) -> Result<OrderDryRunPlan, CliError> {
    let mut preview = batch_create_dry_run_args(client, resolver, args, orders).await?;
    if let Some(args_object) = preview.as_object_mut() {
        args_object.insert(
            "orders_file".to_string(),
            serde_json::json!(args.orders_file),
        );
        args_object.insert(
            "on_behalf_of".to_string(),
            serde_json::json!(args.on_behalf_of),
        );
    }
    Ok(OrderDryRunPlan::new(
        "orders batch-create",
        "submit_order_batch",
        preview,
    ))
}

pub fn twap_create_dry_run_plan(
    resolver: &AssetResolver,
    args: &TwapCreateArgs,
) -> Result<OrderDryRunPlan, CliError> {
    let plan = prepare_twap_create_plan(resolver, args)?;
    let mut preview = plan.dry_run_args(&args.coin, args.dex.as_deref());
    insert_margin_mode(
        &mut preview,
        args.margin_mode.unwrap_or(MarginModeArg::Cross),
    );
    Ok(OrderDryRunPlan::new(
        "orders twap-create",
        "create_twap_order",
        preview,
    ))
}

pub fn twap_cancel_dry_run_plan(
    resolver: &AssetResolver,
    args: &TwapCancelArgs,
) -> Result<OrderDryRunPlan, CliError> {
    let plan = prepare_twap_cancel_plan(resolver, args)?;
    Ok(OrderDryRunPlan::new(
        "orders twap-cancel",
        "cancel_twap_order",
        plan.dry_run_args(&args.coin, args.dex.as_deref()),
    ))
}

pub fn schedule_cancel_dry_run_plan(
    args: &ScheduleCancelArgs,
) -> Result<OrderDryRunPlan, CliError> {
    let plan = prepare_schedule_cancel_plan(args, Utc::now())?;
    Ok(OrderDryRunPlan::new(
        "orders schedule-cancel",
        "schedule_dead_mans_switch",
        plan.dry_run_args(),
    ))
}

pub fn modify_dry_run_plan(args: &ModifyArgs) -> Result<OrderDryRunPlan, CliError> {
    validate_modify_args(args)?;
    Ok(OrderDryRunPlan::new(
        "orders modify",
        "modify_order",
        serde_json::json!({
            "order_id": args.order_id,
            "cloid": args.cloid,
            "price": args.price.map(|value| value.to_string()),
            "trigger_price": args.trigger_price.map(|value| value.to_string()),
            "size": args.size.map(|value| value.to_string()),
        }),
    ))
}

pub fn cancel_dry_run_plan(args: &CancelArgs) -> Result<OrderDryRunPlan, CliError> {
    let identifier = parse_cancel_identifier(args)?;
    Ok(OrderDryRunPlan::new(
        "orders cancel",
        "cancel_order",
        serde_json::json!({
            "order_id": args.order_id,
            "cloid": args.cloid,
            "identifier": identifier.display(),
        }),
    ))
}

pub fn cancel_all_dry_run_plan(
    resolver: &AssetResolver,
    args: &CancelAllArgs,
) -> Result<OrderDryRunPlan, CliError> {
    let coin_filter =
        resolve_cancel_all_coin_filter(resolver, args.coin.as_deref(), args.dex.as_deref())?;
    Ok(OrderDryRunPlan::new(
        "orders cancel-all",
        "cancel_open_orders",
        serde_json::json!({
            "coin": args.coin,
            "dex": args.dex,
            "resolved_coin_filter": coin_filter,
        }),
    ))
}

pub(crate) async fn prepare_cancel_order_plan(
    client: &HttpClient,
    resolver: &AssetResolver,
    user: Address,
    args: &CancelArgs,
) -> Result<CancelOrderPlan, CliError> {
    let identifier = parse_cancel_identifier(args)?;
    let order = lookup_order(client, user, &identifier).await?;
    let asset = asset_index_for_order(client, resolver, &order).await?;

    let action = match &identifier {
        OrderIdentifier::Oid(oid) => Action::Cancel(BatchCancel {
            cancels: vec![Cancel { asset, oid: *oid }],
        }),
        OrderIdentifier::Cloid { parsed, .. } => Action::CancelByCloid(BatchCancelCloid {
            cancels: vec![CancelByCloid {
                asset: asset.try_into().map_err(|_| {
                    CliError::Internal(anyhow::anyhow!(
                        "asset index {asset} does not fit CLOID cancel request"
                    ))
                })?,
                cloid: *parsed,
            }],
        }),
    };

    Ok(CancelOrderPlan {
        action,
        coin: resolver.display_coin(&order.coin),
        order_id: args.order_id,
        cloid: args.cloid.clone(),
    })
}

pub(crate) async fn prepare_create_order_plan(
    client: &HttpClient,
    resolver: &AssetResolver,
    args: &CreateArgs,
) -> Result<CreateOrderPlan, CliError> {
    let prepared = prepare_order(client, resolver, args).await?;
    let mut dry_run_args = prepared_order_dry_run_args(&prepared, args);
    let submission = if create_has_tpsl_legs(args) {
        let prepared_batch = prepare_normal_tpsl_batch(prepared, args)?;
        append_tpsl_preview(&mut dry_run_args, &prepared_batch)?;
        CreateOrderSubmission::NormalTpsl(prepared_batch)
    } else {
        CreateOrderSubmission::Single(prepared)
    };
    Ok(CreateOrderPlan {
        submission,
        dry_run_args,
    })
}

pub(crate) async fn prepare_batch_create_order_plan(
    client: &HttpClient,
    resolver: &AssetResolver,
    args: &BatchCreateArgs,
    orders: Vec<BatchCreateOrder>,
) -> Result<BatchCreateOrderPlan, CliError> {
    let prepared = prepare_batch_create_orders(client, resolver, args, orders).await?;
    let dry_run_args = order_batch_dry_run_args(&prepared);
    Ok(BatchCreateOrderPlan {
        prepared,
        dry_run_args,
    })
}

pub(crate) async fn prepare_modify_order_plan(
    client: &HttpClient,
    resolver: &AssetResolver,
    user: Address,
    args: &ModifyArgs,
) -> Result<ModifyOrderPlan, CliError> {
    validate_modify_args(args)?;
    let identifier = parse_modify_identifier(args)?;
    let order = lookup_order(client, user, &identifier).await?;
    let asset = asset_index_for_order(client, resolver, &order).await?;
    let price = args.price.unwrap_or(order.limit_px);
    let size = args.size.unwrap_or(order.sz);
    let order_type = order_type_placement(&order, args.trigger_price, args.price)?;
    let replacement = OrderRequest {
        asset,
        is_buy: matches!(order.side, Side::Bid),
        limit_px: price,
        sz: size,
        reduce_only: order.reduce_only,
        order_type,
        cloid: order.cloid.unwrap_or_default(),
    };
    Ok(ModifyOrderPlan {
        action: Action::BatchModify(BatchModify {
            modifies: vec![Modify {
                oid: identifier.to_status_identifier(),
                order: replacement,
            }],
        }),
        confirmation: ModifyConfirmation {
            coin: resolver.display_coin(&order.coin),
            status: "modified".to_string(),
            order_id: Some(order.oid),
            cloid: args
                .cloid
                .clone()
                .or_else(|| order.cloid.map(|cloid| format!("{cloid:#x}"))),
            price: price.to_string(),
            size: size.to_string(),
        },
    })
}

pub(crate) async fn prepare_cancel_all_orders_plan(
    client: &HttpClient,
    resolver: &AssetResolver,
    user: Address,
    args: &CancelAllArgs,
) -> Result<CancelAllOrdersPlan, CliError> {
    let open_orders = client
        .open_orders(user, None)
        .await
        .map_err(map_api_error)?;
    let coin_filter =
        resolve_cancel_all_coin_filter(resolver, args.coin.as_deref(), args.dex.as_deref())?;
    let orders = filter_orders_by_coin(open_orders, coin_filter.as_deref());

    let action = if orders.is_empty() {
        None
    } else {
        let mut cancels = Vec::with_capacity(orders.len());
        for order in &orders {
            cancels.push(Cancel {
                asset: asset_index_for_order(client, resolver, order).await?,
                oid: order.oid,
            });
        }
        Some(Action::Cancel(BatchCancel { cancels }))
    };

    Ok(CancelAllOrdersPlan {
        action,
        cancelled_orders: orders.len(),
        summary_coin: args.coin.clone().unwrap_or_else(|| "ALL".to_string()),
    })
}

pub(crate) fn prepare_twap_create_plan(
    resolver: &AssetResolver,
    args: &TwapCreateArgs,
) -> Result<TwapCreatePlan, CliError> {
    validate_twap_create_args(args)?;
    let resolved = resolve_tpsl_perp(resolver, args.dex.as_deref(), &args.coin)?;
    let minutes = duration_seconds_to_minutes(args.duration);
    Ok(TwapCreatePlan {
        action: TwapExchangeAction::TwapOrder {
            twap: TwapOrderAction {
                a: resolved.index,
                b: args.side.is_buy(),
                s: args.size.normalize().to_string(),
                r: false,
                m: minutes,
                t: false,
            },
        },
        coin: resolved.coin,
        asset: resolved.index,
        side: args.side.to_string(),
        size: args.size,
        duration_seconds: args.duration,
        duration_minutes: minutes,
    })
}

pub(crate) fn prepare_twap_cancel_plan(
    resolver: &AssetResolver,
    args: &TwapCancelArgs,
) -> Result<TwapCancelPlan, CliError> {
    let resolved = resolve_tpsl_perp(resolver, args.dex.as_deref(), &args.coin)?;
    Ok(TwapCancelPlan {
        action: TwapExchangeAction::TwapCancel {
            a: resolved.index,
            t: args.twap_id,
        },
        coin: resolved.coin,
        twap_id: args.twap_id,
    })
}

pub(crate) fn prepare_schedule_cancel_plan(
    args: &ScheduleCancelArgs,
    now: chrono::DateTime<Utc>,
) -> Result<ScheduleCancelPlan, CliError> {
    match (args.in_duration, args.clear) {
        (Some(in_duration), false) => {
            let chrono_duration = chrono::Duration::from_std(in_duration)
                .map_err(|err| CliError::Configuration(format!("invalid --in duration: {err}")))?;
            let scheduled_at = now + chrono_duration;
            Ok(ScheduleCancelPlan {
                action: Action::ScheduleCancel(ScheduleCancel {
                    time: Some(scheduled_at.timestamp_millis() as u64),
                }),
                scheduled_at: Some(scheduled_at),
                in_seconds: Some(in_duration.as_secs()),
            })
        }
        (None, true) => Ok(ScheduleCancelPlan {
            action: Action::ScheduleCancel(ScheduleCancel { time: None }),
            scheduled_at: None,
            in_seconds: None,
        }),
        (None, false) => Err(CliError::Configuration(
            "either --in <DURATION> or --clear is required".to_string(),
        )),
        (Some(_), true) => Err(CliError::Configuration(
            "--in and --clear cannot be used together".to_string(),
        )),
    }
}

fn prepared_order_dry_run_args(prepared: &PreparedOrder, args: &CreateArgs) -> serde_json::Value {
    let (is_market, trigger_px, tpsl) = match &prepared.request.order_type {
        OrderTypePlacement::Trigger {
            is_market,
            trigger_px,
            tpsl,
        } => (
            Some(*is_market),
            Some(*trigger_px),
            Some(tpsl_to_str(*tpsl)),
        ),
        OrderTypePlacement::Limit { .. } => (None, None, None),
    };

    let mut value = serde_json::json!({
        "coin": args.coin.clone(),
        "dex": args.dex.clone(),
        "side": args.side.to_string(),
        "type": args.order_type.to_string(),
        "asset_id": prepared.request.asset,
        "resolved_asset": prepared.coin.clone(),
        "limit_px": prepared.request.limit_px.to_string(),
        "trigger_px": trigger_px.map(|value| value.to_string()),
        "is_market": is_market,
        "tpsl": tpsl,
        "size": prepared.request.sz.to_string(),
        "amount": prepared.amount.map(|value| value.to_string()),
        "amount_unit": prepared.amount.as_ref().map(|_| prepared.amount_unit.clone()),
        "tif": prepared.tif.map(|tif| tif.to_string()),
        "reduce_only": prepared.request.reduce_only,
        "cloid": args.cloid.clone(),
        "max_slippage_bps": args.max_slippage_bps,
        "take_profit": args.take_profit.map(|value| value.to_string()),
        "stop_loss": args.stop_loss.map(|value| value.to_string()),
        "grouping": args.grouping.map(|grouping| grouping.to_string()),
        "builder": prepared.builder.as_ref().map(|builder| serde_json::json!({
            "b": builder.b.to_string(),
            "f": builder.f,
        })),
        "builder_address": args.builder.clone(),
        "builder_fee_rate": args.builder_fee_rate.clone(),
    });
    if prepared.asset_kind == TradableAssetKind::Outcome
        && let Some(object) = value.as_object_mut()
        && let Ok(notation) = crate::commands::outcomes::parse_outcome_notation(&args.coin)
    {
        object.insert("outcome".to_string(), serde_json::json!({
            "outcome": notation.outcome,
            "side": notation.side,
            "encoding": notation.encoding,
            "asset_id": prepared.request.asset,
            "notation": args.coin,
            "live_submission": "enabled: outcome order encoding is verified; use explicit --price and --size"
        }));
    }
    if prepared.asset_kind == TradableAssetKind::Perp {
        insert_margin_mode(&mut value, args.margin_mode.unwrap_or(MarginModeArg::Cross));
    }
    value
}

fn order_batch_dry_run_args(prepared: &PreparedOrderBatch) -> serde_json::Value {
    let legs = prepared
        .legs
        .iter()
        .map(|leg| {
            let order = &leg.order;
            let (is_market, trigger_px, tpsl) = match &order.request.order_type {
                OrderTypePlacement::Trigger {
                    is_market,
                    trigger_px,
                    tpsl,
                } => (
                    Some(*is_market),
                    Some(trigger_px.to_string()),
                    Some(tpsl_to_str(*tpsl)),
                ),
                OrderTypePlacement::Limit { .. } => (None, None, None),
            };
            serde_json::json!({
                "leg_index": leg.leg_index,
                "coin": order.coin,
                "side": order.side.to_string(),
                "type": order.order_type.to_string(),
                "asset_id": order.request.asset,
                "resolved_asset": order.coin,
                "limit_px": order.request.limit_px.to_string(),
                "trigger_px": trigger_px,
                "is_market": is_market,
                "tpsl": tpsl,
                "price": order.price.to_string(),
                "size": order.request.sz.to_string(),
                "amount": order.amount.map(|value| value.to_string()),
                "amount_unit": order.amount.as_ref().map(|_| order.amount_unit.clone()),
                "tif": order.tif.map(|tif| tif.to_string()),
                "reduce_only": order.request.reduce_only,
                "cloid": if order.request.cloid == Cloid::default() {
                    None
                } else {
                    Some(order.request.cloid.to_string())
                },
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "grouping": "na",
        "order_count": prepared.legs.len(),
        "orders": legs,
    })
}

fn tpsl_to_str(tpsl: TpSl) -> &'static str {
    match tpsl {
        TpSl::Tp => "tp",
        TpSl::Sl => "sl",
    }
}

/// Build `orders tpsl` dry-run argument details without requiring a signer.
pub fn tpsl_dry_run_preview(
    resolver: &AssetResolver,
    args: &TpslArgs,
) -> Result<serde_json::Value, CliError> {
    validate_tpsl_args(args)?;
    let resolved = resolve_tpsl_perp(resolver, args.dex.as_deref(), &args.coin)?;
    let inferred_side = args
        .side
        .or_else(|| infer_close_side_from_tpsl_prices(args.take_profit, args.stop_loss));
    let mut preview = serde_json::json!({
        "coin": args.coin,
        "dex": args.dex,
        "take_profit": args.take_profit.map(|value| value.to_string()),
        "stop_loss": args.stop_loss.map(|value| value.to_string()),
        "grouping": args.grouping.to_string(),
        "grouping_wire": args.grouping.wire_value(),
        "resolved_asset": resolved.coin,
        "side": inferred_side.map(|side| side.to_string()),
        "size": args.size.map(|value| value.to_string()),
        "size_mode": if args.size.is_some() { "fixed" } else { "current_position" },
    });
    insert_margin_mode(
        &mut preview,
        args.margin_mode.unwrap_or(MarginModeArg::Cross),
    );

    if let (Some(side), Some(size)) = (args.side, args.size) {
        validate_tpsl_price_ordering(side, args.take_profit, args.stop_loss, "orders tpsl")?;
        let parsed_cloid = args.cloid.as_deref().map(parse_cloid).transpose()?;
        let prepared = build_position_tpsl_batch(
            TpslGroupingArg::PositionTpsl,
            resolved.coin,
            resolved.index,
            resolved.collateral,
            side,
            size,
            args.take_profit,
            args.stop_loss,
            parsed_cloid,
        );
        append_tpsl_preview(&mut preview, &prepared)?;
    } else {
        append_position_sized_tpsl_preview(
            &mut preview,
            resolved.index,
            inferred_side,
            args.take_profit,
            args.stop_loss,
        );
    }

    Ok(preview)
}

pub fn tpsl_dry_run_plan(
    resolver: &AssetResolver,
    args: &TpslArgs,
) -> Result<OrderDryRunPlan, CliError> {
    Ok(OrderDryRunPlan::new(
        "orders tpsl",
        "submit_position_tpsl",
        tpsl_dry_run_preview(resolver, args)?,
    ))
}

fn append_tpsl_preview(
    preview: &mut serde_json::Value,
    prepared: &PreparedTpslBatch,
) -> Result<(), CliError> {
    let mut parent = None;
    let mut children = Vec::new();
    let mut legs = Vec::new();

    for leg in &prepared.legs {
        let order = serde_json::to_value(&leg.request)
            .map_err(|err| CliError::Internal(anyhow::anyhow!("serialize TP/SL leg: {err}")))?;
        let entry = serde_json::json!({
            "leg": leg.leg.to_string(),
            "coin": leg.coin,
            "side": leg.side.to_string(),
            "type": leg.order_type.to_string(),
            "price": leg.price.to_string(),
            "size": leg.size.to_string(),
            "tif": leg.tif.map(|tif| tif.to_string()),
            "reduce_only": leg.reduce_only,
            "warning": leg.warning,
            "order": order,
        });
        if matches!(leg.leg, TpslLegKind::Parent) {
            parent = Some(entry.clone());
        } else {
            children.push(entry.clone());
        }
        legs.push(entry);
    }

    let object = preview
        .as_object_mut()
        .ok_or_else(|| CliError::Internal(anyhow::anyhow!("dry-run preview must be an object")))?;
    object.insert(
        "batch_grouping".to_string(),
        serde_json::json!(prepared.grouping.to_string()),
    );
    object.insert(
        "batch_grouping_wire".to_string(),
        serde_json::json!(prepared.grouping.wire_value()),
    );
    object.insert(
        "batch_order".to_string(),
        serde_json::to_value(&prepared.batch)
            .map_err(|err| CliError::Internal(anyhow::anyhow!("serialize TP/SL batch: {err}")))?,
    );
    object.insert("legs".to_string(), serde_json::Value::Array(legs));
    object.insert(
        "tpsl_children".to_string(),
        serde_json::Value::Array(children),
    );
    if let Some(parent) = parent {
        object.insert("parent_order".to_string(), parent);
    }
    Ok(())
}

fn append_position_sized_tpsl_preview(
    preview: &mut serde_json::Value,
    asset: usize,
    inferred_side: Option<OrderSide>,
    take_profit: Option<Decimal>,
    stop_loss: Option<Decimal>,
) {
    let mut position_tp_order = serde_json::Value::Null;
    let mut position_sl_order = serde_json::Value::Null;
    let mut orders = Vec::new();
    let mut legs = Vec::new();
    let mut children = Vec::new();
    for (leg, price, tpsl) in [
        (TpslLegKind::TakeProfit, take_profit, "tp"),
        (TpslLegKind::StopLoss, stop_loss, "sl"),
    ] {
        let Some(price) = price else {
            continue;
        };
        let side_value = inferred_side.map(|side| side.to_string());
        let child = serde_json::json!({
            "asset": asset,
            "is_buy": inferred_side.map(OrderSide::is_buy),
            "limit_px": "0",
            "sz": null,
            "reduce_only": true,
            "trigger_px": price.to_string(),
            "is_market": true,
            "tpsl": tpsl,
        });
        match leg {
            TpslLegKind::TakeProfit => position_tp_order = child.clone(),
            TpslLegKind::StopLoss => position_sl_order = child.clone(),
            TpslLegKind::Parent => {}
        }
        legs.push(serde_json::json!({
            "leg": leg.to_string(),
            "side": side_value,
            "type": match leg {
                TpslLegKind::TakeProfit => "take-profit",
                TpslLegKind::StopLoss => "stop-loss",
                TpslLegKind::Parent => "parent",
            },
            "price": price.to_string(),
            "size": "current_position",
            "reduce_only": true,
            "grouping": TpslGroupingArg::PositionTpsl.to_string(),
            "grouping_wire": TpslGroupingArg::PositionTpsl.wire_value(),
            "order": child.clone(),
        }));
        children.push(child.clone());
        orders.push(serde_json::json!({
            "a": asset,
            "b": inferred_side.map(OrderSide::is_buy),
            "p": price.to_string(),
            "s": "current_position",
            "r": true,
            "t": {
                "trigger": {
                    "isMarket": true,
                    "triggerPx": price.to_string(),
                    "tpsl": tpsl,
                }
            }
        }));
    }

    if let Some(object) = preview.as_object_mut() {
        object.insert(
            "batch_grouping".to_string(),
            serde_json::json!(TpslGroupingArg::PositionTpsl.to_string()),
        );
        object.insert(
            "batch_grouping_wire".to_string(),
            serde_json::json!(TpslGroupingArg::PositionTpsl.wire_value()),
        );
        object.insert("position_size_lookup".to_string(), serde_json::json!(true));
        object.insert(
            "position_lookup_note".to_string(),
            serde_json::json!("live submission resolves current position size before signing"),
        );
        object.insert("legs".to_string(), serde_json::Value::Array(legs));
        object.insert(
            "tpsl_children".to_string(),
            serde_json::Value::Array(children.clone()),
        );
        object.insert(
            "batch_order_preview".to_string(),
            serde_json::json!({
                "grouping": TpslGroupingArg::PositionTpsl.wire_value(),
                "orders": orders,
                "size_source": "current_position",
            }),
        );
        object.insert("position_tp_order".to_string(), position_tp_order);
        object.insert("position_sl_order".to_string(), position_sl_order);
    }
}

fn infer_close_side_from_tpsl_prices(
    take_profit: Option<Decimal>,
    stop_loss: Option<Decimal>,
) -> Option<OrderSide> {
    match (take_profit, stop_loss) {
        (Some(tp), Some(sl)) if tp > sl => Some(OrderSide::Sell),
        (Some(tp), Some(sl)) if tp < sl => Some(OrderSide::Buy),
        _ => None,
    }
}
