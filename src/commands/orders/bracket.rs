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
    resolver: &crate::assets::AssetResolver,
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

/// The entry order of a bracket as the user can name it in a command.
#[derive(Clone, Copy)]
struct EntryRef {
    oid: Option<u64>,
    cloid: Cloid,
}

impl EntryRef {
    fn label(&self) -> String {
        self.oid
            .map_or_else(|| format!("{:#x}", self.cloid), |oid| oid.to_string())
    }

    fn cancel_command(&self, args: &BracketArgs) -> String {
        let target = self.oid.map_or_else(
            || format!("--cloid {:#x}", self.cloid),
            |oid| oid.to_string(),
        );
        format!(
            "hyperliquid orders cancel {target}{}",
            on_behalf_of_flag(args)
        )
    }
}

impl EntryTracking {
    fn entry(&self) -> EntryRef {
        EntryRef {
            oid: self.oid,
            cloid: self.cloid,
        }
    }
}

fn on_behalf_of_flag(args: &BracketArgs) -> String {
    args.on_behalf_of
        .as_deref()
        .map(|selector| format!(" --on-behalf-of {selector}"))
        .unwrap_or_default()
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
            "Can't place a bracket on mainnet without --yes. Rerun with --yes once you approve it:\n    \
             hyperliquid --format json orders bracket --coin ETH --side buy --size 0.1 --take-profit +10% --stop-loss -5% --yes"
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

    let statuses = place_prepared_single(context, prepared, &entry_args, vault_address).await?;
    let entry_status = statuses.into_iter().next().ok_or_else(|| {
        CliError::Internal(anyhow::anyhow!("bracket entry returned no order status"))
    })?;

    let mut tracking = match entry_status {
        OrderResponseStatus::Filled { total_sz, oid, .. } => {
            let entry = EntryRef {
                oid: Some(oid),
                cloid,
            };
            return arm_protection(
                context,
                args,
                vault_address,
                format,
                started,
                &entry,
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
        // Accepted without an oid. The exchange has not reported a fill, so the entry
        // is tracked by cloid and polled like any other resting entry.
        OrderResponseStatus::Success
        | OrderResponseStatus::WaitingForTrigger
        | OrderResponseStatus::WaitingForFill => EntryTracking {
            oid: None,
            cloid,
            asset,
            open_size: requested_size,
        },
        OrderResponseStatus::Error(err) => {
            return Err(CliError::Unsupported(format!(
                "Couldn't place the bracket entry. Exchange said: {}",
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
        let open = match context.client.open_orders(user, None).await {
            Ok(open) => open,
            Err(err) => {
                return Err(report_unprotected(
                    args,
                    &tracking.entry(),
                    "resting",
                    "Couldn't check it",
                    &crate::commands::map_api_error(err),
                    format,
                    started,
                ));
            }
        };
        let open_size = entry_open_size(&open, &tracking);
        if open_size <= Decimal::ZERO {
            let filled = requested_size - tracking.open_size;
            return arm_protection(
                context,
                args,
                vault_address,
                format,
                started,
                &tracking.entry(),
                filled,
                "armed",
            )
            .await;
        }
        tracking.open_size = open_size;
    }

    // Entry timed out: cancel the remainder so it cannot fill unprotected,
    // then protect whatever already filled.
    if let Err(err) = cancel_entry(context, &tracking, vault_address).await {
        return Err(report_unprotected(
            args,
            &tracking.entry(),
            "resting",
            "Couldn't cancel it after the entry timeout",
            &err,
            format,
            started,
        ));
    }
    let filled = requested_size - tracking.open_size;
    if filled > Decimal::ZERO {
        arm_protection(
            context,
            args,
            vault_address,
            format,
            started,
            &tracking.entry(),
            filled,
            "entry_cancelled_protected",
        )
        .await
    } else {
        let headline = format!(
            "Entry order {} timed out unfilled and was cancelled. No position was opened.",
            tracking.entry().label()
        );
        let mut result = BracketOutput::result("entry_cancelled", args, "0".to_string(), started);
        result.problem = Some(BracketProblem {
            entry_order_id: tracking.oid,
            headline: headline.clone(),
            next_commands: Vec::new(),
        });
        output::print_data(&result, format, started.elapsed());
        Err(CliError::PartialResults(headline).into())
    }
}

/// Print the bracket result for an entry that is still live or filled without
/// protection, and return the error (exit 15) that tells the user how to
/// recover. In JSON the printed result is the only document.
fn report_unprotected(
    args: &BracketArgs,
    entry: &EntryRef,
    state: &str,
    failed_step: &str,
    cause: &CliError,
    format: OutputFormat,
    started: Instant,
) -> anyhow::Error {
    let headline = format!(
        "Entry order {} is {state} and not protected. {failed_step}: {cause}",
        entry.label()
    );
    let mut next_commands = vec!["hyperliquid orders open".to_string()];
    let lead = if state == "resting" {
        next_commands.push(entry.cancel_command(args));
        "Check what is live, then cancel the entry if you don't want it:"
    } else {
        next_commands.push(format!(
            "hyperliquid orders tpsl --coin {} --take-profit {} --stop-loss {}{}",
            args.coin,
            args.take_profit.display(),
            args.stop_loss.display(),
            on_behalf_of_flag(args)
        ));
        "Check what is live, then protect the position:"
    };
    let status = if state == "resting" {
        "entry_open_unprotected"
    } else {
        "entry_filled_unprotected"
    };
    let mut result = BracketOutput::result(status, args, "0".to_string(), started);
    result.problem = Some(BracketProblem {
        entry_order_id: entry.oid,
        headline: headline.clone(),
        next_commands: next_commands.clone(),
    });
    output::print_data(&result, format, started.elapsed());
    let commands = next_commands
        .iter()
        .map(|command| format!("    {command}"))
        .collect::<Vec<_>>()
        .join("\n");
    CliError::PartialResults(format!("{headline}\n\n{lead}\n{commands}")).into()
}

#[allow(clippy::too_many_arguments)]
async fn arm_protection(
    context: OrderExecutionContext<'_>,
    args: &BracketArgs,
    vault_address: Option<Address>,
    format: OutputFormat,
    started: Instant,
    entry: &EntryRef,
    filled: Decimal,
    status: &str,
) -> Result<(), anyhow::Error> {
    if filled <= Decimal::ZERO {
        let headline = format!(
            "Entry order {} is no longer open and nothing filled. No position was opened.",
            entry.label()
        );
        let mut result = BracketOutput::result("entry_unfilled", args, "0".to_string(), started);
        result.problem = Some(BracketProblem {
            entry_order_id: entry.oid,
            headline: headline.clone(),
            next_commands: vec!["hyperliquid orders open".to_string()],
        });
        output::print_data(&result, format, started.elapsed());
        return Err(CliError::PartialResults(format!(
            "{headline}\n\nCheck what is live:\n    hyperliquid orders open"
        ))
        .into());
    }
    if let Err(err) = super::tpsl(
        context,
        &protection_tpsl_args(args, Some(filled)),
        vault_address,
        OutputFormat::Json,
        false,
    )
    .await
    {
        return Err(report_unprotected(
            args,
            entry,
            "filled",
            "Couldn't place the take-profit and stop-loss",
            &crate::errors::from_anyhow(err),
            format,
            started,
        ));
    }
    output::print_data(
        &BracketOutput::result(status, args, filled.to_string(), started),
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
            fast: false,
        }),
        None => Action::CancelByCloid(BatchCancelCloid {
            cancels: vec![CancelByCloid {
                asset: tracking.asset,
                cloid: tracking.cloid,
            }],
            fast: false,
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
    /// Set when the bracket ended with work left on the exchange.
    problem: Option<BracketProblem>,
}

/// What went wrong after the entry order was placed, and what to run next.
struct BracketProblem {
    entry_order_id: Option<u64>,
    headline: String,
    next_commands: Vec<String>,
}

impl BracketOutput {
    fn result(status: &str, args: &BracketArgs, protected_size: String, started: Instant) -> Self {
        Self {
            status: status.to_string(),
            coin: args.coin.clone(),
            protected_size,
            elapsed_ms: elapsed_ms(started),
            problem: None,
        }
    }
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

    /// One document. A problem adds the standard error keys, so this result is
    /// also the error envelope and no second document is printed.
    fn to_json_value(&self) -> serde_json::Value {
        let mut value = json!({
            "kind": "result",
            "status": self.status,
            "coin": self.coin,
            "protected_size": self.protected_size,
            "elapsed_ms": self.elapsed_ms,
        });
        if let (Some(problem), Some(object)) = (&self.problem, value.as_object_mut()) {
            object.insert("error".to_string(), json!(problem.headline));
            object.insert("category".to_string(), json!("partial_failure"));
            object.insert("exit_code".to_string(), json!(15));
            object.insert("entry_order_id".to_string(), json!(problem.entry_order_id));
            object.insert("next_commands".to_string(), json!(problem.next_commands));
        }
        value
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
            problem: None,
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
