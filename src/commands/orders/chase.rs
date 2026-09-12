use std::io::IsTerminal;
use std::time::{Duration, Instant};

use hypersdk::hypercore::types::{Action, BasicOrder, BatchOrder, OrderGrouping, Side};
use hypersdk::hypercore::{Chain, HttpClient};
use hypersdk::{Address, Decimal};
use serde_json::json;

use crate::commands::map_api_error;
use crate::errors::CliError;
use crate::output::{self, OutputFormat, TableData};
use crate::signing::SelectedSigner;

use super::planning::{
    CreateOrderSubmission, prepare_cancel_all_orders_plan, prepare_create_order_plan,
};
use super::{
    CancelAllArgs, ChaseArgs, CreateArgs, CreateOrderType, OrderExecutionContext, OrderSide, TifArg,
};
use crate::commands::actions;

const BPS: i64 = 10_000;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
const MIN_NOTIONAL_USDC: i64 = 10;

pub fn chase_dry_run_plan(
    args: &ChaseArgs,
    mid: Option<Decimal>,
) -> Result<serde_json::Value, CliError> {
    validate_chase_args(args)?;
    let timeout = effective_timeout(args)?;
    let first_price = mid.map(|mid| quote_price(mid, args.side, args.offset));
    Ok(json!({
        "coin": args.coin,
        "dex": args.dex,
        "side": args.side.to_string(),
        "size": args.size.to_string(),
        "offset_bps": args.offset,
        "timeout_secs": timeout.as_secs(),
        "interval_secs": args.interval.as_secs(),
        "max_chase_bps": args.max_chase,
        "tif": "alo",
        "reduce_only": args.reduce_only,
        "first_price": first_price.map(|price| price.to_string()),
        "would_execute": "chase",
    }))
}

pub async fn chase(
    context: OrderExecutionContext<'_>,
    args: &ChaseArgs,
    vault_address: Option<Address>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    validate_chase_args(args)?;
    let timeout = effective_timeout(args)?;
    if context.submission.require_mainnet_confirmation && !args.yes {
        return Err(CliError::Configuration(
            "Mainnet chase confirmation required; rerun with --yes for deliberate automation.\n  \
             hyperliquid --format json -y orders chase --coin ETH --side buy --size 0.5 --timeout 60s"
                .to_string(),
        )
        .into());
    }

    let started = Instant::now();
    let start_mid = fetch_mid(context.client, &args.coin).await?;
    let mut remaining = args.size;
    let mut status = "timeout";
    let user = vault_address.unwrap_or_else(|| context.submission.signer.query_address());

    while started.elapsed() < timeout && remaining > Decimal::ZERO {
        let mid = fetch_mid(context.client, &args.coin).await?;
        if start_mid > Decimal::ZERO {
            let traveled_bps = ((mid - start_mid).abs() / start_mid) * Decimal::from(BPS);
            if traveled_bps > Decimal::from(args.max_chase) {
                status = "max_chase";
                let _ = cancel_working_orders(context, user, args, vault_address).await;
                break;
            }
        }
        if remaining * mid < Decimal::from(MIN_NOTIONAL_USDC) {
            status = "min_notional";
            let _ = cancel_working_orders(context, user, args, vault_address).await;
            break;
        }

        let _ = cancel_working_orders(context, user, args, vault_address).await;
        let price = quote_price(mid, args.side, args.offset);
        place_chase_quote(context, args, remaining, price, vault_address).await?;
        tokio::time::sleep(args.interval.max(Duration::from_millis(200))).await;
        remaining = remaining_from_open_orders(context.client, user, args, remaining).await?;
    }

    if remaining <= Decimal::ZERO {
        status = "filled";
        remaining = Decimal::ZERO;
    }

    output::print_data(
        &ChaseOutput {
            status: status.to_string(),
            remaining: remaining.to_string(),
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        },
        format,
        started.elapsed(),
    );
    Ok(())
}

fn validate_chase_args(args: &ChaseArgs) -> Result<(), CliError> {
    if args.size <= Decimal::ZERO {
        return Err(CliError::Configuration(
            "orders chase --size must be greater than zero.\n  \
             hyperliquid --format json --dry-run orders chase --coin ETH --side buy --size 0.5 --timeout 60s"
                .to_string(),
        ));
    }
    Ok(())
}

fn effective_timeout(args: &ChaseArgs) -> Result<Duration, CliError> {
    match args.timeout {
        Some(timeout) if !timeout.is_zero() => Ok(timeout),
        Some(_) => Err(CliError::Configuration(
            "orders chase --timeout must be greater than zero.\n  \
             hyperliquid --format json --dry-run orders chase --coin ETH --side buy --size 0.5 --timeout 60s"
                .to_string(),
        )),
        None if agent_mode() => Err(CliError::Configuration(
            "orders chase requires --timeout in agent mode.\n  \
             hyperliquid --format json --dry-run orders chase --coin ETH --side buy --size 0.5 --timeout 60s"
                .to_string(),
        )),
        None => Ok(DEFAULT_TIMEOUT),
    }
}

fn agent_mode() -> bool {
    !std::io::stdout().is_terminal()
        || std::env::var("HYPERLIQUID_AGENT").is_ok_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
}

fn chase_create_args(args: &ChaseArgs, size: Decimal, price: Decimal) -> CreateArgs {
    CreateArgs {
        coin: args.coin.clone(),
        dex: args.dex.clone(),
        side: args.side,
        price: Some(price),
        trigger_price: None,
        size: Some(size),
        amount: None,
        order_type: CreateOrderType::Limit,
        tif: TifArg::Alo,
        reduce_only: args.reduce_only,
        max_slippage_bps: 50,
        take_profit: None,
        stop_loss: None,
        grouping: None,
        on_behalf_of: args.on_behalf_of.clone(),
        margin_mode: None,
        builder: None,
        builder_fee_rate: None,
        cloid: None,
        yes: true,
    }
}

fn quote_price(mid: Decimal, side: OrderSide, offset_bps: u32) -> Decimal {
    let offset = mid * Decimal::from(offset_bps) / Decimal::from(BPS);
    match side {
        OrderSide::Buy => mid - offset,
        OrderSide::Sell => mid + offset,
    }
}

async fn fetch_mid(client: &HttpClient, coin: &str) -> Result<Decimal, CliError> {
    let mids = client.all_mids(None).await.map_err(map_api_error)?;
    mids.get(coin)
        .copied()
        .ok_or_else(|| CliError::Unsupported(format!("no mid price for {coin}")))
}

async fn remaining_from_open_orders(
    client: &HttpClient,
    user: Address,
    args: &ChaseArgs,
    previous: Decimal,
) -> Result<Decimal, CliError> {
    let open = client
        .open_orders(user, None)
        .await
        .map_err(map_api_error)?;
    let working = working_size(&open, args);
    if working > Decimal::ZERO {
        Ok(working)
    } else {
        let _ = previous;
        Ok(Decimal::ZERO)
    }
}

fn working_size(orders: &[BasicOrder], args: &ChaseArgs) -> Decimal {
    orders
        .iter()
        .filter(|order| order_matches_chase(order, args))
        .map(|order| order.sz)
        .fold(Decimal::ZERO, |total, size| total + size)
}

fn order_matches_chase(order: &BasicOrder, args: &ChaseArgs) -> bool {
    let coin = args
        .dex
        .as_ref()
        .map(|dex| format!("{dex}:{}", args.coin))
        .unwrap_or_else(|| args.coin.clone());
    let same_coin =
        order.coin.eq_ignore_ascii_case(&args.coin) || order.coin.eq_ignore_ascii_case(&coin);
    let same_side = match args.side {
        OrderSide::Buy => matches!(order.side, Side::Bid),
        OrderSide::Sell => matches!(order.side, Side::Ask),
    };
    same_coin && same_side
}

async fn cancel_working_orders(
    context: OrderExecutionContext<'_>,
    user: Address,
    args: &ChaseArgs,
    vault_address: Option<Address>,
) -> Result<(), CliError> {
    let plan = prepare_cancel_all_orders_plan(
        context.client,
        context.resolver,
        user,
        &CancelAllArgs {
            coin: Some(args.coin.clone()),
            dex: args.dex.clone(),
            on_behalf_of: args.on_behalf_of.clone(),
            yes: true,
        },
    )
    .await?;
    if let Some(action) = plan.action {
        submit_action(
            context.submission.api_base_url,
            context.submission.chain,
            context.submission.signer,
            action,
            vault_address,
            "chase cancel failed",
        )
        .await?;
    }
    Ok(())
}

async fn place_chase_quote(
    context: OrderExecutionContext<'_>,
    args: &ChaseArgs,
    size: Decimal,
    price: Decimal,
    vault_address: Option<Address>,
) -> Result<(), anyhow::Error> {
    let plan = prepare_create_order_plan(
        context.client,
        context.resolver,
        &chase_create_args(args, size, price),
    )
    .await?;
    let prepared = match plan.submission {
        CreateOrderSubmission::Single(prepared) => prepared,
        CreateOrderSubmission::NormalTpsl(_) => {
            return Err(CliError::Internal(anyhow::anyhow!(
                "chase quotes cannot attach TP/SL children"
            ))
            .into());
        }
    };
    submit_action(
        context.submission.api_base_url,
        context.submission.chain,
        context.submission.signer,
        Action::Order(BatchOrder {
            orders: vec![prepared.request],
            grouping: OrderGrouping::Na,
        }),
        vault_address,
        "chase quote failed",
    )
    .await?;
    Ok(())
}

async fn submit_action(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    action: Action,
    vault_address: Option<Address>,
    error_label: &'static str,
) -> Result<(), CliError> {
    actions::send_l1_action_raw(
        api_base_url,
        chain,
        signer,
        action,
        actions::nonce_now(),
        vault_address,
        error_label,
    )
    .await?;
    Ok(())
}

struct ChaseOutput {
    status: String,
    remaining: String,
    elapsed_ms: u64,
}

impl TableData for ChaseOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Status", "Remaining", "Elapsed ms"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.status.clone(),
            self.remaining.clone(),
            self.elapsed_ms.to_string(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        json!({
            "kind": "result",
            "status": self.status,
            "remaining": self.remaining,
            "elapsed_ms": self.elapsed_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_args(timeout: Option<Duration>) -> ChaseArgs {
        ChaseArgs {
            coin: "ETH".to_string(),
            dex: None,
            side: OrderSide::Buy,
            size: Decimal::new(5, 1),
            offset: 5,
            timeout,
            interval: Duration::from_secs(2),
            max_chase: 100,
            reduce_only: false,
            on_behalf_of: None,
            yes: true,
        }
    }

    #[test]
    fn dry_run_includes_bounds_and_first_price() {
        let plan = chase_dry_run_plan(
            &sample_args(Some(Duration::from_secs(60))),
            Some(Decimal::from(100)),
        )
        .unwrap();
        assert_eq!(plan["would_execute"], "chase");
        assert_eq!(plan["offset_bps"], 5);
        assert_eq!(plan["timeout_secs"], 60);
        assert_eq!(plan["max_chase_bps"], 100);
        assert_eq!(plan["first_price"], "99.95");
        assert_eq!(plan["tif"], "alo");
    }

    #[test]
    fn quote_buy_is_below_mid() {
        assert_eq!(
            quote_price(Decimal::from(100), OrderSide::Buy, 5),
            Decimal::new(9995, 2)
        );
    }

    #[test]
    fn chase_result_is_one_json_object() {
        let output = ChaseOutput {
            status: "filled".to_string(),
            remaining: "0".to_string(),
            elapsed_ms: 12,
        };
        let value = output.to_json_value();
        assert!(value.is_object());
        assert_eq!(value["kind"], "result");
        assert!(
            serde_json::from_str::<serde_json::Value>(&value.to_string()).is_ok(),
            "chase result must parse as one JSON value"
        );
    }
}
