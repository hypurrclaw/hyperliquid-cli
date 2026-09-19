use std::io::IsTerminal;
use std::time::{Duration, Instant};

use hypersdk::hypercore::types::{
    Action, BasicOrder, BatchCancelCloid, BatchOrder, CancelByCloid, OrderGrouping,
    OrderResponseStatus,
};
use hypersdk::hypercore::{Chain, Cloid, HttpClient};
use hypersdk::{Address, Decimal};
use serde_json::json;

use crate::commands::map_api_error;
use crate::errors::CliError;
use crate::output::{self, OutputFormat, TableData};
use crate::response_sanitization::labelled_untrusted_text;
use crate::signing::SelectedSigner;

use super::planning::{CreateOrderSubmission, prepare_create_order_plan};
use super::{
    ChaseArgs, CreateArgs, CreateOrderType, OrderExecutionContext, OrderSide, TifArg,
    qualify_dex_asset,
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

/// A live chase quote identified by its exchange OID and/or client order ID.
struct WorkingQuote {
    oid: Option<u64>,
    cloid: Cloid,
    asset: u32,
    /// Remaining open size of this quote.
    size: Decimal,
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
    let coin_key = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let start_mid = fetch_mid(context.client, &coin_key).await?;
    let mut remaining = args.size;
    let mut status = "timeout";
    let mut working: Option<WorkingQuote> = None;
    let mut seq: u64 = 0;
    let user = vault_address.unwrap_or_else(|| context.submission.signer.query_address());

    let outcome: Result<(), CliError> = async {
        while started.elapsed() < timeout && remaining > Decimal::ZERO {
            let mid = fetch_mid(context.client, &coin_key).await?;
            if start_mid > Decimal::ZERO {
                let traveled_bps = ((mid - start_mid).abs() / start_mid) * Decimal::from(BPS);
                if traveled_bps > Decimal::from(args.max_chase) {
                    status = "max_chase";
                    break;
                }
            }
            if remaining * mid < Decimal::from(MIN_NOTIONAL_USDC) {
                status = "min_notional";
                break;
            }

            // Requote: cancel only the quote this chase placed, never other
            // orders resting on the same market.
            if let Some(quote) = working.take() {
                cancel_working_quote(context, &quote, vault_address).await?;
            }
            seq += 1;
            let cloid = chase_cloid(seq)?;
            let price = quote_price(mid, args.side, args.offset);
            let (asset, quote_status) =
                place_chase_quote(context, args, remaining, price, cloid, vault_address).await?;
            match quote_status {
                OrderResponseStatus::Resting { oid, .. } => {
                    working = Some(WorkingQuote {
                        oid: Some(oid),
                        cloid,
                        asset,
                        size: remaining,
                    });
                }
                OrderResponseStatus::Success => {
                    working = Some(WorkingQuote {
                        oid: None,
                        cloid,
                        asset,
                        size: remaining,
                    });
                }
                OrderResponseStatus::Filled { total_sz, .. } => {
                    remaining = (remaining - total_sz).max(Decimal::ZERO);
                }
                OrderResponseStatus::Error(err) => {
                    status = "rejected";
                    return Err(CliError::Unsupported(format!(
                        "chase quote rejected: {}",
                        labelled_untrusted_text(&err)
                    )));
                }
            }

            tokio::time::sleep(args.interval.max(Duration::from_millis(200))).await;

            // Reconcile the working quote against open orders so partial fills
            // shrink the next quote and a vanished quote counts as filled.
            if let Some(quote) = working.as_mut() {
                let open = client_open_orders(context.client, user).await?;
                let open_size = quote_open_size(&open, quote);
                if open_size <= Decimal::ZERO {
                    remaining = (remaining - quote.size).max(Decimal::ZERO);
                    working = None;
                } else if open_size < quote.size {
                    remaining = (remaining - (quote.size - open_size)).max(Decimal::ZERO);
                    quote.size = open_size;
                }
            }
        }
        Ok(())
    }
    .await;

    // Final cleanup: never leave a live chase quote behind on timeout, bound
    // breaks, or errors. Cleanup failures are propagated, not swallowed.
    if let Some(quote) = working.take()
        && let Err(cleanup) = cancel_working_quote(context, &quote, vault_address).await
    {
        return Err(match outcome {
            Ok(()) => cleanup.into(),
            Err(err) => anyhow::anyhow!("{err}; chase cleanup also failed: {cleanup}"),
        });
    }
    outcome?;

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

/// Unique client order ID for one chase quote so requotes and cleanup only
/// ever touch orders this chase placed.
fn chase_cloid(seq: u64) -> Result<Cloid, CliError> {
    super::generated_cloid(seq)
}

fn chase_create_args(args: &ChaseArgs, size: Decimal, price: Decimal, cloid: Cloid) -> CreateArgs {
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
        cloid: Some(format!("{cloid:#x}")),
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

async fn fetch_mid(client: &HttpClient, coin_key: &str) -> Result<Decimal, CliError> {
    let mids = client.all_mids(None).await.map_err(map_api_error)?;
    mids.get(coin_key)
        .copied()
        .ok_or_else(|| CliError::Unsupported(format!("no mid price for {coin_key}")))
}

async fn client_open_orders(
    client: &HttpClient,
    user: Address,
) -> Result<Vec<BasicOrder>, CliError> {
    client.open_orders(user, None).await.map_err(map_api_error)
}

/// Open size of a specific chase quote, matched by OID or cloid only.
fn quote_open_size(orders: &[BasicOrder], quote: &WorkingQuote) -> Decimal {
    orders
        .iter()
        .filter(|order| {
            quote.oid.is_some_and(|oid| order.oid == oid)
                || order.cloid.is_some_and(|cloid| cloid == quote.cloid)
        })
        .map(|order| order.sz)
        .fold(Decimal::ZERO, |total, size| total + size)
}

/// Cancel one chase quote by cloid; unrelated orders are untouched.
async fn cancel_working_quote(
    context: OrderExecutionContext<'_>,
    quote: &WorkingQuote,
    vault_address: Option<Address>,
) -> Result<(), CliError> {
    submit_action(
        context.submission.api_base_url,
        context.submission.chain,
        context.submission.signer,
        Action::CancelByCloid(BatchCancelCloid {
            cancels: vec![CancelByCloid {
                asset: quote.asset,
                cloid: quote.cloid,
            }],
        }),
        vault_address,
        "chase cancel failed",
    )
    .await?;
    Ok(())
}

async fn place_chase_quote(
    context: OrderExecutionContext<'_>,
    args: &ChaseArgs,
    size: Decimal,
    price: Decimal,
    cloid: Cloid,
    vault_address: Option<Address>,
) -> Result<(u32, OrderResponseStatus), anyhow::Error> {
    let plan = prepare_create_order_plan(
        context.client,
        context.resolver,
        &chase_create_args(args, size, price, cloid),
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
    let asset = u32::try_from(prepared.request.asset).map_err(|_| {
        CliError::Internal(anyhow::anyhow!(
            "chase quote asset index {} does not fit cancel-by-cloid",
            prepared.request.asset
        ))
    })?;
    let response = submit_action(
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
    let statuses = super::parse_order_statuses(response)?;
    let status = statuses.into_iter().next().ok_or_else(|| {
        CliError::Internal(anyhow::anyhow!(
            "chase quote response returned no order status"
        ))
    })?;
    Ok((asset, status))
}

async fn submit_action(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    action: Action,
    vault_address: Option<Address>,
    error_label: &'static str,
) -> Result<serde_json::Value, CliError> {
    actions::send_l1_action_raw(
        api_base_url,
        chain,
        signer,
        action,
        actions::nonce_now(),
        vault_address,
        error_label,
    )
    .await
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
    fn chase_cloids_are_unique_per_sequence() {
        let first = chase_cloid(1).unwrap();
        let second = chase_cloid(2).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn quote_open_size_matches_only_tracked_quote() {
        use hypersdk::hypercore::types::{OrderType, Side};

        fn order(oid: u64, cloid: Option<Cloid>, sz: Decimal) -> BasicOrder {
            BasicOrder {
                timestamp: 0,
                coin: "ETH".to_string(),
                side: Side::Bid,
                limit_px: Decimal::from(100),
                sz,
                oid,
                orig_sz: sz,
                cloid,
                order_type: OrderType::Limit,
                tif: None,
                reduce_only: false,
            }
        }

        let cloid = chase_cloid(7).unwrap();
        let quote = WorkingQuote {
            oid: Some(42),
            cloid,
            asset: 0,
            size: Decimal::ONE,
        };
        let orders = vec![
            order(42, Some(cloid), Decimal::new(3, 1)),
            order(99, None, Decimal::from(5)),
        ];
        assert_eq!(quote_open_size(&orders, &quote), Decimal::new(3, 1));
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
