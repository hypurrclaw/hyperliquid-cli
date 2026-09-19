use std::time::{Duration, Instant};

use hypersdk::Address;
use hypersdk::Decimal;
use hypersdk::hypercore::Cloid;
use hypersdk::hypercore::types::{
    Action, BasicOrder, BatchCancel, BatchCancelCloid, Cancel, CancelByCloid, OrderResponseStatus,
};
use serde_json::json;

use crate::commands::actions;
use crate::errors::CliError;
use crate::output::{self, OutputFormat, TableData};
use crate::response_sanitization::labelled_untrusted_text;

use super::planning::{
    CreateOrderSubmission, prepare_create_order_plan, resolve_trigger_prices,
    validate_tpsl_price_ordering,
};
use super::{
    BracketArgs, CreateArgs, CreateOrderType, OrderExecutionContext, TifArg, TpslArgs,
    place_prepared_single, validate_tpsl_args,
};

const DEFAULT_ENTRY_TIMEOUT: Duration = Duration::from_secs(300);

pub fn validate_bracket_args(args: &BracketArgs) -> Result<(), CliError> {
    match args.entry {
        CreateOrderType::Market => {
            if args.price.is_some() {
                return Err(CliError::Configuration(
                    "orders bracket --entry market does not accept --price".to_string(),
                ));
            }
            if args.size.is_none() && args.amount.is_none() {
                return Err(CliError::Configuration(
                    "orders bracket requires --size or --amount for market entry.\n  \
                     hyperliquid --format json --dry-run orders bracket --coin ETH --side buy --size 0.1 --take-profit +10% --stop-loss -5%"
                        .to_string(),
                ));
            }
            if args.size.is_some() && args.amount.is_some() {
                return Err(CliError::Configuration(
                    "orders bracket accepts --size or --amount, not both".to_string(),
                ));
            }
        }
        CreateOrderType::Limit => {
            if args.price.is_none() || args.size.is_none() {
                return Err(CliError::Configuration(
                    "orders bracket --entry limit requires --price and --size.\n  \
                     hyperliquid --format json --dry-run orders bracket --coin ETH --side buy --entry limit --price 3000 --size 0.1 --take-profit +10% --stop-loss -5%"
                        .to_string(),
                ));
            }
        }
        other => {
            return Err(CliError::Configuration(format!(
                "orders bracket --entry supports market or limit, not {other}"
            )));
        }
    }
    Ok(())
}

pub async fn bracket_dry_run_plan(
    client: &hypersdk::hypercore::HttpClient,
    resolver: &crate::commands::AssetResolver,
    args: &BracketArgs,
) -> Result<serde_json::Value, CliError> {
    validate_bracket_args(args)?;
    let entry =
        super::create_dry_run_preview(client, resolver, &entry_create_args(args, None)).await?;
    let protection = super::tpsl_dry_run_preview(resolver, &protection_tpsl_args(args, args.size))?;
    Ok(json!({
        "coin": args.coin,
        "dex": args.dex,
        "side": args.side.to_string(),
        "entry": args.entry.to_string(),
        "entry_timeout_secs": args.entry_timeout.as_secs(),
        "take_profit": args.take_profit.display(),
        "stop_loss": args.stop_loss.display(),
        "would_execute": "bracket",
        "entry_plan": entry,
        "protection_plan": protection,
    }))
}

/// A resting bracket entry tracked by OID and/or cloid.
struct EntryTracking {
    oid: Option<u64>,
    cloid: Cloid,
    asset: u32,
    /// Latest known open size of the entry order.
    open_size: Decimal,
}

pub async fn bracket(
    context: OrderExecutionContext<'_>,
    args: &BracketArgs,
    vault_address: Option<Address>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    validate_bracket_args(args)?;
    if context.submission.require_mainnet_confirmation && !args.yes {
        return Err(CliError::Configuration(
            "Mainnet bracket confirmation required; rerun with --yes for deliberate automation.\n  \
             hyperliquid --format json -y orders bracket --coin ETH --side buy --size 0.1 --take-profit +10% --stop-loss -5%"
                .to_string(),
        )
        .into());
    }

    let started = Instant::now();
    let cloid = super::generated_cloid(1)?;
    let entry_args = entry_create_args(args, Some(cloid));
    let plan = prepare_create_order_plan(context.client, context.resolver, &entry_args).await?;
    let prepared = match plan.submission {
        CreateOrderSubmission::Single(prepared) => prepared,
        CreateOrderSubmission::NormalTpsl(_) => {
            return Err(CliError::Internal(anyhow::anyhow!(
                "bracket entries cannot attach TP/SL children"
            ))
            .into());
        }
    };
    let asset = u32::try_from(prepared.request.asset).map_err(|_| {
        CliError::Internal(anyhow::anyhow!(
            "bracket entry asset index {} does not fit cancel-by-cloid",
            prepared.request.asset
        ))
    })?;
    let requested_size = prepared.size;

    // Fail fast before the entry executes: the protection legs must be
    // constructible and correctly ordered for the position side.
    validate_tpsl_args(&protection_tpsl_args(args, Some(requested_size)))?;
    let (est_tp, est_sl) = resolve_trigger_prices(
        Some(&args.take_profit),
        Some(&args.stop_loss),
        Some(prepared.price),
        args.side,
    )?;
    validate_tpsl_price_ordering(args.side.opposite(), est_tp, est_sl, "orders bracket")?;

    let statuses =
        place_prepared_single(context, prepared, &entry_args, vault_address, format).await?;
    let entry_status = statuses.into_iter().next().ok_or_else(|| {
        CliError::Internal(anyhow::anyhow!("bracket entry returned no order status"))
    })?;

    let mut tracking = match entry_status {
        OrderResponseStatus::Filled { total_sz, .. } => {
            return arm_protection(
                context,
                args,
                vault_address,
                format,
                started,
                total_sz,
                "armed",
            )
            .await;
        }
        OrderResponseStatus::Resting { oid, .. } => EntryTracking {
            oid: Some(oid),
            cloid,
            asset,
            open_size: requested_size,
        },
        OrderResponseStatus::Success => EntryTracking {
            oid: None,
            cloid,
            asset,
            open_size: requested_size,
        },
        OrderResponseStatus::Error(err) => {
            return Err(CliError::Unsupported(format!(
                "bracket entry rejected: {}",
                labelled_untrusted_text(&err)
            ))
            .into());
        }
    };

    // Poll the entry order itself (not position presence) so pre-existing
    // exposure cannot falsely complete the bracket.
    let timeout = if args.entry_timeout.is_zero() {
        DEFAULT_ENTRY_TIMEOUT
    } else {
        args.entry_timeout
    };
    let user = vault_address.unwrap_or_else(|| context.submission.signer.query_address());
    while started.elapsed() < timeout {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let open = context
            .client
            .open_orders(user, None)
            .await
            .map_err(crate::commands::map_api_error)?;
        let open_size = entry_open_size(&open, &tracking);
        if open_size <= Decimal::ZERO {
            let filled = requested_size - tracking.open_size;
            return arm_protection(
                context,
                args,
                vault_address,
                format,
                started,
                filled,
                "armed",
            )
            .await;
        }
        tracking.open_size = open_size;
    }

    // Entry timed out: cancel the remainder so it cannot fill unprotected,
    // then protect whatever already filled.
    cancel_entry(context, &tracking, vault_address).await?;
    let filled = requested_size - tracking.open_size;
    if filled > Decimal::ZERO {
        arm_protection(
            context,
            args,
            vault_address,
            format,
            started,
            filled,
            "entry_cancelled_protected",
        )
        .await
    } else {
        output::print_data(
            &BracketOutput {
                status: "entry_cancelled".to_string(),
                coin: args.coin.clone(),
                protected_size: "0".to_string(),
                elapsed_ms: elapsed_ms(started),
            },
            format,
            started.elapsed(),
        );
        Err(CliError::PartialResults("entry_cancelled".to_string()).into())
    }
}

async fn arm_protection(
    context: OrderExecutionContext<'_>,
    args: &BracketArgs,
    vault_address: Option<Address>,
    format: OutputFormat,
    started: Instant,
    filled: Decimal,
    status: &str,
) -> Result<(), anyhow::Error> {
    if filled <= Decimal::ZERO {
        output::print_data(
            &BracketOutput {
                status: "entry_unfilled".to_string(),
                coin: args.coin.clone(),
                protected_size: "0".to_string(),
                elapsed_ms: elapsed_ms(started),
            },
            format,
            started.elapsed(),
        );
        return Err(CliError::PartialResults("entry_unfilled".to_string()).into());
    }
    super::tpsl(
        context,
        &protection_tpsl_args(args, Some(filled)),
        vault_address,
        OutputFormat::Json,
        false,
    )
    .await?;
    output::print_data(
        &BracketOutput {
            status: status.to_string(),
            coin: args.coin.clone(),
            protected_size: filled.to_string(),
            elapsed_ms: elapsed_ms(started),
        },
        format,
        started.elapsed(),
    );
    Ok(())
}

fn entry_create_args(args: &BracketArgs, cloid: Option<Cloid>) -> CreateArgs {
    CreateArgs {
        coin: args.coin.clone(),
        dex: args.dex.clone(),
        side: args.side,
        price: args.price,
        trigger_price: None,
        size: args.size,
        amount: args.amount,
        order_type: args.entry,
        tif: TifArg::Gtc,
        reduce_only: false,
        max_slippage_bps: args.max_slippage_bps,
        take_profit: None,
        stop_loss: None,
        grouping: None,
        on_behalf_of: args.on_behalf_of.clone(),
        margin_mode: None,
        builder: None,
        builder_fee_rate: None,
        cloid: cloid.map(|cloid| format!("{cloid:#x}")),
        yes: true,
    }
}

/// TP/SL args for the filled size. `size` is `None` only in dry-run previews,
/// where the live position size is used instead.
fn protection_tpsl_args(args: &BracketArgs, size: Option<Decimal>) -> TpslArgs {
    TpslArgs {
        coin: args.coin.clone(),
        dex: args.dex.clone(),
        take_profit: Some(args.take_profit.clone()),
        stop_loss: Some(args.stop_loss.clone()),
        grouping: super::PositionTpslGroupingArg::PositionTpsl,
        side: size.map(|_| args.side.opposite()),
        size,
        on_behalf_of: args.on_behalf_of.clone(),
        margin_mode: None,
        yes: true,
        cloid: None,
    }
}

/// Open size of the tracked entry order, matched by OID or cloid only.
fn entry_open_size(orders: &[BasicOrder], tracking: &EntryTracking) -> Decimal {
    orders
        .iter()
        .filter(|order| {
            tracking.oid.is_some_and(|oid| order.oid == oid)
                || order.cloid.is_some_and(|cloid| cloid == tracking.cloid)
        })
        .map(|order| order.sz)
        .fold(Decimal::ZERO, |total, size| total + size)
}

/// Cancel the remaining entry order so a timed-out bracket cannot fill
/// unprotected later.
async fn cancel_entry(
    context: OrderExecutionContext<'_>,
    tracking: &EntryTracking,
    vault_address: Option<Address>,
) -> Result<(), CliError> {
    let action = match tracking.oid {
        Some(oid) => Action::Cancel(BatchCancel {
            cancels: vec![Cancel {
                asset: tracking.asset as usize,
                oid,
            }],
        }),
        None => Action::CancelByCloid(BatchCancelCloid {
            cancels: vec![CancelByCloid {
                asset: tracking.asset,
                cloid: tracking.cloid,
            }],
        }),
    };
    actions::send_l1_action_raw(
        context.submission.api_base_url,
        context.submission.chain,
        context.submission.signer,
        action,
        actions::nonce_now(),
        vault_address,
        "bracket entry cancel failed",
    )
    .await?;
    Ok(())
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

struct BracketOutput {
    status: String,
    coin: String,
    protected_size: String,
    elapsed_ms: u64,
}
impl TableData for BracketOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Status", "Coin", "Protected Size", "Elapsed ms"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.status.clone(),
            self.coin.clone(),
            self.protected_size.clone(),
            self.elapsed_ms.to_string(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        json!({
            "kind": "result",
            "status": self.status,
            "coin": self.coin,
            "protected_size": self.protected_size,
            "elapsed_ms": self.elapsed_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;
    use crate::commands::orders::{OrderSide, TriggerPriceSpec};

    fn sample_args() -> BracketArgs {
        BracketArgs {
            coin: "ETH".to_string(),
            dex: None,
            side: OrderSide::Buy,
            size: Some(Decimal::new(1, 1)),
            amount: None,
            entry: CreateOrderType::Market,
            price: None,
            take_profit: TriggerPriceSpec::Percent(Decimal::from(10)),
            stop_loss: TriggerPriceSpec::Percent(Decimal::from(-5)),
            entry_timeout: Duration::from_secs(300),
            on_behalf_of: None,
            max_slippage_bps: 500,
            yes: true,
        }
    }

    #[test]
    fn market_entry_requires_size_or_amount() {
        let mut args = sample_args();
        args.size = None;
        assert!(validate_bracket_args(&args).is_err());
    }

    #[test]
    fn limit_entry_requires_price() {
        let mut args = sample_args();
        args.entry = CreateOrderType::Limit;
        assert!(validate_bracket_args(&args).is_err());
        args.price = Some(Decimal::from(100));
        assert!(validate_bracket_args(&args).is_ok());
    }

    #[test]
    fn amount_entry_protection_uses_filled_size() {
        // --amount entries have no --size; protection must be built from the
        // confirmed filled size, not the (absent) size flag.
        let mut args = sample_args();
        args.size = None;
        args.amount = Some(Decimal::from(100));
        let tpsl = protection_tpsl_args(&args, Some(Decimal::new(25, 3)));
        assert_eq!(tpsl.size, Some(Decimal::new(25, 3)));
        assert_eq!(tpsl.side, Some(OrderSide::Sell));
        assert!(validate_tpsl_args(&tpsl).is_ok());
    }

    #[test]
    fn short_bracket_percent_triggers_resolve_favorably() {
        // Short entry at 100: +10% TP resolves to 90, -5% SL to 105.
        let mut args = sample_args();
        args.side = OrderSide::Sell;
        let (tp, sl) = resolve_trigger_prices(
            Some(&args.take_profit),
            Some(&args.stop_loss),
            Some(Decimal::from(100)),
            args.side,
        )
        .unwrap();
        assert_eq!(tp, Some(Decimal::from(90)));
        assert_eq!(sl, Some(Decimal::from(105)));
        assert!(
            validate_tpsl_price_ordering(args.side.opposite(), tp, sl, "orders bracket").is_ok()
        );
    }

    #[test]
    fn bracket_result_is_one_json_object() {
        let output = BracketOutput {
            status: "armed".to_string(),
            coin: "ETH".to_string(),
            protected_size: "0.1".to_string(),
            elapsed_ms: 8,
        };
        let value = output.to_json_value();
        assert!(value.is_object());
        assert_eq!(value["kind"], "result");
        assert!(
            serde_json::from_str::<serde_json::Value>(&value.to_string()).is_ok(),
            "bracket result must parse as one JSON value"
        );
    }
}
