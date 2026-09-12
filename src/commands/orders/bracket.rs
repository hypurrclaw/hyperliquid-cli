use std::time::{Duration, Instant};

use hypersdk::Address;
use serde_json::json;

use crate::errors::CliError;
use crate::output::{self, OutputFormat, TableData};

use super::{
    BracketArgs, CreateArgs, CreateOrderType, OrderExecutionContext, TifArg, TpslArgs, create,
    create_dry_run_preview, tpsl, tpsl_dry_run_preview,
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
    let entry = create_dry_run_preview(client, resolver, &entry_create_args(args)).await?;
    let protection = tpsl_dry_run_preview(resolver, &protection_tpsl_args(args))?;
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
    create(
        context,
        &entry_create_args(args),
        vault_address,
        OutputFormat::Json,
        false,
    )
    .await?;

    if args.entry == CreateOrderType::Limit {
        let filled = wait_for_entry_fill(context, args, vault_address, started).await?;
        if !filled {
            output::print_data(
                &BracketOutput {
                    status: "entry_resting_unprotected".to_string(),
                    coin: args.coin.clone(),
                    elapsed_ms: elapsed_ms(started),
                },
                format,
                started.elapsed(),
            );
            return Err(CliError::PartialResults("entry_resting_unprotected".to_string()).into());
        }
    }

    tpsl(
        context,
        &protection_tpsl_args(args),
        vault_address,
        OutputFormat::Json,
        false,
    )
    .await?;

    output::print_data(
        &BracketOutput {
            status: "armed".to_string(),
            coin: args.coin.clone(),
            elapsed_ms: elapsed_ms(started),
        },
        format,
        started.elapsed(),
    );
    Ok(())
}

fn entry_create_args(args: &BracketArgs) -> CreateArgs {
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
        cloid: None,
        yes: true,
    }
}

fn protection_tpsl_args(args: &BracketArgs) -> TpslArgs {
    TpslArgs {
        coin: args.coin.clone(),
        dex: args.dex.clone(),
        take_profit: Some(args.take_profit.clone()),
        stop_loss: Some(args.stop_loss.clone()),
        grouping: super::PositionTpslGroupingArg::PositionTpsl,
        side: Some(args.side.opposite()),
        size: args.size,
        on_behalf_of: args.on_behalf_of.clone(),
        margin_mode: None,
        yes: true,
        cloid: None,
    }
}

async fn wait_for_entry_fill(
    context: OrderExecutionContext<'_>,
    args: &BracketArgs,
    vault_address: Option<Address>,
    started: Instant,
) -> Result<bool, CliError> {
    let timeout = if args.entry_timeout.is_zero() {
        DEFAULT_ENTRY_TIMEOUT
    } else {
        args.entry_timeout
    };
    let user = vault_address.unwrap_or_else(|| context.submission.signer.query_address());
    while started.elapsed() < timeout {
        let state = context
            .client
            .clearinghouse_state(user, args.dex.clone())
            .await
            .map_err(crate::commands::map_api_error)?;
        if state.asset_positions.iter().any(|position| {
            position.position.coin.eq_ignore_ascii_case(&args.coin)
                && !position.position.szi.is_zero()
        }) {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Ok(false)
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

struct BracketOutput {
    status: String,
    coin: String,
    elapsed_ms: u64,
}

impl TableData for BracketOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Status", "Coin", "Elapsed ms"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.status.clone(),
            self.coin.clone(),
            self.elapsed_ms.to_string(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        json!({
            "kind": "result",
            "status": self.status,
            "coin": self.coin,
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
    fn bracket_result_is_one_json_object() {
        let output = BracketOutput {
            status: "armed".to_string(),
            coin: "ETH".to_string(),
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
