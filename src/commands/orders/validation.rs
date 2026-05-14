use super::planning::create_has_tpsl_legs;
use super::*;

/// Validate `orders create` arguments before network placement or signer resolution.
pub fn validate_create_args(args: &CreateArgs) -> Result<(), CliError> {
    if let Some(cloid) = args.cloid.as_deref() {
        parse_cloid(cloid)?;
    }
    validate_create_order_type_flags(args)?;
    validate_positive(args.price, "price")?;
    validate_positive(args.trigger_price, "trigger-price")?;
    validate_positive(args.size, "size")?;
    validate_positive(args.amount, "amount")?;
    validate_positive(args.take_profit, "take-profit")?;
    validate_positive(args.stop_loss, "stop-loss")?;
    validate_market_slippage(args.max_slippage_bps)?;
    validate_create_tpsl_flags(args)?;
    validate_builder_fee_args(args)?;

    match args.order_type {
        CreateOrderType::Limit => {
            require_decimals(
                &[(args.price, "--price"), (args.size, "--size")],
                "limit orders",
            )?;
        }
        CreateOrderType::StopLoss | CreateOrderType::TakeProfit => {
            require_market_trigger_price(args)?;
            require_decimals(&[(args.size, "--size")], "market trigger orders")?;
        }
        CreateOrderType::StopLimit | CreateOrderType::TakeLimit => {
            require_decimals(
                &[
                    (args.trigger_price, "--trigger-price"),
                    (args.price, "--price"),
                    (args.size, "--size"),
                ],
                "limit trigger orders",
            )?;
        }
        CreateOrderType::Market => {
            require_decimal(args.amount, "--amount", "market orders")?;
        }
    }

    Ok(())
}

/// Validate `orders scale` arguments before auth or network submission.
pub fn validate_scale_args(args: &ScaleArgs) -> Result<(), CliError> {
    validate_positive(Some(args.start_price), "start-price")?;
    validate_positive(Some(args.end_price), "end-price")?;
    validate_positive(Some(args.total_size), "total-size")?;
    if args.order_count == 0 {
        return Err(CliError::Configuration(
            "orders scale requires --orders greater than zero".to_string(),
        ));
    }
    if args.order_count > MAX_BATCH_ORDER_COUNT {
        return Err(CliError::Configuration(format!(
            "orders scale supports at most {MAX_BATCH_ORDER_COUNT} orders"
        )));
    }
    if args.order_count > 1 && args.start_price == args.end_price {
        return Err(CliError::Configuration(
            "orders scale requires different start and end prices when --orders is greater than one"
                .to_string(),
        ));
    }
    Ok(())
}

/// Validate `orders batch-create` arguments before auth or network submission.
pub fn validate_batch_create_args(args: &BatchCreateArgs) -> Result<(), CliError> {
    if args.orders_file.as_os_str().is_empty() {
        return Err(CliError::Configuration(
            "orders batch-create requires --orders-file".to_string(),
        ));
    }
    Ok(())
}

/// Read and validate a batch-create file before signer resolution.
pub fn read_validated_batch_create_orders(
    args: &BatchCreateArgs,
) -> Result<Vec<BatchCreateOrder>, CliError> {
    let orders = read_batch_create_orders(&args.orders_file)?;
    if orders.is_empty() {
        return Err(CliError::Configuration(
            "orders batch-create requires at least one order leg".to_string(),
        ));
    }
    Ok(orders)
}

/// Validate `orders tpsl` arguments before auth or network submission.
pub fn validate_tpsl_args(args: &TpslArgs) -> Result<(), CliError> {
    if let Some(cloid) = args.cloid.as_deref() {
        parse_cloid(cloid)?;
    }
    validate_positive(args.take_profit, "take-profit")?;
    validate_positive(args.stop_loss, "stop-loss")?;
    validate_positive(args.size, "size")?;
    if args.take_profit.is_none() && args.stop_loss.is_none() {
        return Err(CliError::Configuration(
            "orders tpsl requires --take-profit or --stop-loss".to_string(),
        ));
    }
    if let (Some(take_profit), Some(stop_loss)) = (args.take_profit, args.stop_loss)
        && take_profit == stop_loss
    {
        return Err(CliError::Configuration(
            "orders tpsl --take-profit and --stop-loss cannot be equal".to_string(),
        ));
    }
    match (args.side, args.size) {
        (Some(_), None) => Err(CliError::Configuration(
            "orders tpsl --side requires --size for fixed-size TP/SL orders".to_string(),
        )),
        (None, Some(_)) => Err(CliError::Configuration(
            "orders tpsl --size requires --side so the close direction is explicit".to_string(),
        )),
        _ => Ok(()),
    }
}

/// Validate asset-specific `orders create` constraints once metadata resolution is available.
pub fn validate_create_resolved_asset(
    args: &CreateArgs,
    asset: &ResolvedAsset,
) -> Result<(), CliError> {
    if matches!(asset, ResolvedAsset::Spot { .. }) {
        reject_spot_margin_mode(args.margin_mode)?;
        reject_spot_reduce_only(args)?;
        reject_spot_trigger_order(args)?;
        reject_spot_tpsl_grouping(args)?;
    }

    Ok(())
}

/// Validate `orders modify` arguments before network placement or signer resolution.
pub fn validate_modify_args(args: &ModifyArgs) -> Result<(), CliError> {
    parse_modify_identifier(args)?;
    validate_positive(args.price, "price")?;
    validate_positive(args.trigger_price, "trigger-price")?;
    validate_positive(args.size, "size")?;
    if args.price.is_none() && args.trigger_price.is_none() && args.size.is_none() {
        return Err(CliError::Configuration(
            "orders modify requires --price, --trigger-price, or --size".to_string(),
        ));
    }
    Ok(())
}

/// Validate `orders twap-create` arguments before auth or network submission.
pub fn validate_twap_create_args(args: &TwapCreateArgs) -> Result<(), CliError> {
    validate_positive(Some(args.size), "size")?;
    if args.duration < 300 {
        return Err(CliError::Configuration(
            "TWAP duration must be at least 300 seconds and a whole-minute value in seconds (for example, --duration 300, --duration 600, or --duration 3600)"
                .to_string(),
        ));
    }
    if !args.duration.is_multiple_of(60) {
        return Err(CliError::Configuration(
            "TWAP duration must be a whole-minute value in seconds; use values like --duration 300, --duration 600, or --duration 3600"
                .to_string(),
        ));
    }
    Ok(())
}

fn validate_create_order_type_flags(args: &CreateArgs) -> Result<(), CliError> {
    match args.order_type {
        CreateOrderType::Limit if args.amount.is_some() => Err(CliError::Configuration(
            "orders create --type limit uses --price and --size; remove --amount or use --type market --amount for amount-based market orders"
                .to_string(),
        )),
        CreateOrderType::Limit if args.trigger_price.is_some() => Err(CliError::Configuration(
            "orders create --type limit uses --price and --size; remove --trigger-price or use --type stop-limit/take-limit for trigger-limit orders"
                .to_string(),
        )),
        CreateOrderType::Market if args.trigger_price.is_some() => Err(CliError::Configuration(
            "orders create --type market uses --amount; remove --trigger-price or use --type stop-loss/take-profit for market trigger orders, or --type stop-limit/take-limit with --price and --size"
                .to_string(),
        )),
        CreateOrderType::Market if args.price.is_some() || args.size.is_some() => {
            let incompatible = incompatible_price_size_flags(args);
            Err(CliError::Configuration(format!(
                "orders create --type market uses --amount; remove {incompatible} or use --type limit with --price and --size"
            )))
        }
        CreateOrderType::StopLoss | CreateOrderType::TakeProfit if args.amount.is_some() => {
            Err(CliError::Configuration(format!(
                "orders create --type {} uses --trigger-price (or legacy --price) and --size for market trigger orders; remove --amount",
                args.order_type
            )))
        }
        CreateOrderType::StopLoss | CreateOrderType::TakeProfit
            if args.price.is_some() && args.trigger_price.is_some() =>
        {
            Err(CliError::Configuration(format!(
                "orders create --type {} uses --trigger-price or legacy --price as the trigger price; remove one price flag or use --type stop-limit/take-limit for trigger-limit orders",
                args.order_type
            )))
        }
        CreateOrderType::StopLimit | CreateOrderType::TakeLimit if args.amount.is_some() => {
            Err(CliError::Configuration(format!(
                "orders create --type {} uses --trigger-price, --price, and --size for trigger-limit orders; remove --amount",
                args.order_type
            )))
        }
        _ => Ok(()),
    }
}

fn validate_builder_fee_args(args: &CreateArgs) -> Result<(), CliError> {
    match (args.builder.as_deref(), args.builder_fee_rate.as_deref()) {
        (Some(raw_builder), Some(raw_fee)) => {
            builder::parse_builder_address(raw_builder)?;
            builder::validate_max_fee_rate(raw_fee)?;
            Ok(())
        }
        (Some(_), None) => Err(CliError::Configuration(
            "orders create --builder requires --builder-fee-rate".to_string(),
        )),
        (None, Some(_)) => Err(CliError::Configuration(
            "orders create --builder-fee-rate requires --builder".to_string(),
        )),
        (None, None) => Ok(()),
    }
}

fn validate_create_tpsl_flags(args: &CreateArgs) -> Result<(), CliError> {
    let has_legs = create_has_tpsl_legs(args);
    match (has_legs, args.grouping) {
        (false, Some(_)) => Err(CliError::Configuration(
            "orders create --grouping requires --take-profit or --stop-loss".to_string(),
        )),
        (true, None) => Err(CliError::Configuration(
            "orders create --take-profit/--stop-loss require --grouping normal-tpsl".to_string(),
        )),
        _ => {
            if has_legs && args.order_type.is_trigger() {
                return Err(CliError::Configuration(
                    "orders create TP/SL children require --type limit or --type market; use --type stop-loss or --type take-profit only for standalone trigger orders"
                        .to_string(),
                ));
            }
            Ok(())
        }
    }
}

fn reject_spot_margin_mode(margin_mode: Option<MarginModeArg>) -> Result<(), CliError> {
    if let Some(margin_mode) = margin_mode {
        return Err(CliError::Configuration(format!(
            "orders create --margin-mode {} is only supported for perpetual orders",
            margin_mode
        )));
    }
    Ok(())
}

fn incompatible_price_size_flags(args: &CreateArgs) -> &'static str {
    match (
        args.price.is_some(),
        args.trigger_price.is_some(),
        args.size.is_some(),
    ) {
        (true, true, true) => "--price, --trigger-price, and --size",
        (true, true, false) => "--price and --trigger-price",
        (true, false, true) => "--price and --size",
        (false, true, true) => "--trigger-price and --size",
        (true, false, false) => "--price",
        (false, true, false) => "--trigger-price",
        (false, false, true) => "--size",
        (false, false, false) => "--price/--trigger-price/--size",
    }
}

pub(super) fn reject_spot_reduce_only(args: &CreateArgs) -> Result<(), CliError> {
    if args.reduce_only {
        return Err(CliError::Unsupported(
            "orders create --reduce-only is currently supported for perpetual markets only; remove --reduce-only for spot orders"
                .to_string(),
        ));
    }

    Ok(())
}

pub(super) fn reject_spot_trigger_order(args: &CreateArgs) -> Result<(), CliError> {
    if args.order_type.is_trigger() {
        return Err(CliError::Unsupported(
            "orders create trigger orders currently support perpetual markets only".to_string(),
        ));
    }

    Ok(())
}

pub(super) fn reject_spot_tpsl_grouping(args: &CreateArgs) -> Result<(), CliError> {
    if create_has_tpsl_legs(args) {
        return Err(CliError::Unsupported(
            "orders create TP/SL grouping currently supports perpetual markets only".to_string(),
        ));
    }

    Ok(())
}

pub(super) fn require_decimal(
    value: Option<Decimal>,
    flag: &'static str,
    context: &'static str,
) -> Result<Decimal, CliError> {
    value.ok_or_else(|| {
        CliError::Configuration(format!("orders create requires {flag} for {context}"))
    })
}

fn require_decimals(
    values: &[(Option<Decimal>, &'static str)],
    context: &'static str,
) -> Result<(), CliError> {
    let missing = values
        .iter()
        .filter_map(|(value, flag)| value.is_none().then_some(*flag))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(CliError::Configuration(format!(
            "orders create requires {} for {context}",
            missing.join(" and ")
        )))
    }
}

fn require_market_trigger_price(args: &CreateArgs) -> Result<(), CliError> {
    if args.trigger_price.is_none() && args.price.is_none() {
        return Err(CliError::Configuration(
            "orders create requires --trigger-price or --price for market trigger orders"
                .to_string(),
        ));
    }
    Ok(())
}

fn validate_positive(value: Option<Decimal>, name: &'static str) -> Result<(), CliError> {
    if let Some(value) = value
        && value <= Decimal::ZERO
    {
        let message = match name {
            "price" => "price must be positive",
            "trigger-price" => "trigger-price must be positive",
            "size" => "size must be greater than zero",
            "amount" => "amount must be greater than zero",
            "take-profit" => "take-profit must be positive",
            "stop-loss" => "stop-loss must be positive",
            _ => "value must be greater than zero",
        };
        return Err(CliError::Configuration(message.to_string()));
    }
    Ok(())
}

fn validate_market_slippage(max_slippage_bps: u16) -> Result<(), CliError> {
    if !(MIN_MARKET_ORDER_SLIPPAGE_BPS..=MAX_MARKET_ORDER_SLIPPAGE_BPS).contains(&max_slippage_bps)
    {
        return Err(CliError::Configuration(format!(
            "--max-slippage-bps must be between {MIN_MARKET_ORDER_SLIPPAGE_BPS} and {MAX_MARKET_ORDER_SLIPPAGE_BPS}"
        )));
    }
    Ok(())
}

pub(crate) fn parse_relative_duration(input: &str) -> Result<Duration, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("duration cannot be empty".to_string());
    }

    let (number_part, multiplier, unit_name) = match trimmed.chars().last().unwrap() {
        's' | 'S' => (&trimmed[..trimmed.len() - 1], 1_u64, "seconds"),
        'm' | 'M' => (&trimmed[..trimmed.len() - 1], 60_u64, "minutes"),
        'h' | 'H' => (&trimmed[..trimmed.len() - 1], 60 * 60, "hours"),
        'd' | 'D' => (&trimmed[..trimmed.len() - 1], 24 * 60 * 60, "days"),
        ch if ch.is_ascii_digit() => (trimmed, 1_u64, "seconds"),
        _ => {
            return Err(
                "duration must be a positive integer with optional s/m/h/d suffix".to_string(),
            );
        }
    };

    let value = number_part
        .parse::<u64>()
        .map_err(|_| format!("duration {unit_name} must be a positive integer"))?;
    if value == 0 {
        return Err("duration must be greater than zero".to_string());
    }
    value
        .checked_mul(multiplier)
        .map(Duration::from_secs)
        .ok_or_else(|| "duration is too large".to_string())
}
