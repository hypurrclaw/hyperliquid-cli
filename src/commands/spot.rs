/// Spot market commands.
///
/// Commands:
/// - `hyperliquid spot list` — list all spot markets
/// - `hyperliquid spot get <PAIR>` — get details for a specific spot pair
use clap::Args;
use hypersdk::Decimal;
use hypersdk::hypercore::PriceTick;
use serde::Serialize;

use crate::assets::{SpotListing, load_spot_markets};
use crate::command_spec::{CliCommand, CommandSpec, GroupSpec, InputKind, NoArgs, arg};
use crate::errors::CliError;
use crate::http_api::ApiClient;
use crate::invocation::Invocation;
use crate::output::TableData;

#[derive(Args, Debug)]
pub struct SpotGetArgs {
    /// Spot pair (e.g., PURR/USDC, HYPE/USDC)
    pub pair: String,
}

/// Renderable spot market row.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SpotMarketRow {
    pub symbol: String,
    pub index: usize,
    pub base: String,
    pub quote: String,
    pub base_sz_decimals: i64,
    pub quote_sz_decimals: i64,
    pub tick_size_at_price_1: Option<String>,
}

impl From<SpotListing> for SpotMarketRow {
    fn from(market: SpotListing) -> Self {
        Self {
            symbol: market.symbol,
            index: market.index,
            base: market.base,
            quote: market.quote,
            base_sz_decimals: market.base_sz_decimals,
            quote_sz_decimals: market.quote_sz_decimals,
            tick_size_at_price_1: PriceTick::for_spot(market.base_sz_decimals)
                .tick_for(Decimal::from(1))
                .map(|tick| tick.to_string()),
        }
    }
}

/// Output wrapper for spot markets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotMarketsOutput {
    markets: Vec<SpotMarketRow>,
}

impl SpotMarketsOutput {
    #[must_use]
    pub fn new(markets: Vec<SpotMarketRow>) -> Self {
        Self { markets }
    }
}

impl TableData for SpotMarketsOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Symbol",
            "Index",
            "Base",
            "Quote",
            "Base Sz Decimals",
            "Quote Sz Decimals",
            "Tick Size @ Price 1",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.markets
            .iter()
            .map(|market| {
                vec![
                    market.symbol.clone(),
                    market.index.to_string(),
                    market.base.clone(),
                    market.quote.clone(),
                    market.base_sz_decimals.to_string(),
                    market.quote_sz_decimals.to_string(),
                    market
                        .tick_size_at_price_1
                        .clone()
                        .unwrap_or_else(|| "n/a".to_string()),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.markets).unwrap_or_else(|_| serde_json::json!([]))
    }

    fn empty_message(&self) -> Option<&str> {
        Some("No spot markets.")
    }
}

/// Output wrapper for a single spot market.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotMarketDetailsOutput {
    market: SpotMarketRow,
}

impl SpotMarketDetailsOutput {
    #[must_use]
    pub fn new(market: SpotMarketRow) -> Self {
        Self { market }
    }
}

impl TableData for SpotMarketDetailsOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Symbol",
            "Index",
            "Base",
            "Quote",
            "Base Sz Decimals",
            "Quote Sz Decimals",
            "Tick Size @ Price 1",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        SpotMarketsOutput::new(vec![self.market.clone()]).rows()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.market).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// Fetch and render all spot markets.
pub async fn list(inv: &Invocation) -> Result<(), anyhow::Error> {
    let markets = load_spot_market_rows(inv.remote()?.api()).await?;
    inv.print(&SpotMarketsOutput::new(markets));
    Ok(())
}

/// Resolve, fetch, and render one spot market.
pub async fn get(inv: &Invocation, pair: &str) -> Result<(), anyhow::Error> {
    let api = inv.remote()?.api();
    let resolver = inv.asset_resolver().await?;
    let resolved = resolver.resolve_spot(pair).map_err(CliError::from)?;
    let (resolved_symbol, resolved_index) = (resolved.symbol, resolved.index);

    let market = load_spot_market_rows(api)
        .await?
        .into_iter()
        .find(|market| {
            market.index == resolved_index || market.symbol.eq_ignore_ascii_case(&resolved_symbol)
        })
        .ok_or_else(|| CliError::AssetNotFoundNoSuggestion {
            asset: pair.to_string(),
        })?;

    inv.print(&SpotMarketDetailsOutput::new(market));
    Ok(())
}

async fn load_spot_market_rows(api: &ApiClient) -> Result<Vec<SpotMarketRow>, CliError> {
    Ok(load_spot_markets(api)
        .await?
        .into_iter()
        .map(SpotMarketRow::from)
        .collect())
}

pub const GROUP: GroupSpec = GroupSpec::new(&["spot"], "Query spot markets");

pub struct SpotList;

impl CliCommand for SpotList {
    type Args = NoArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["spot", "list"],
        "market",
        "List all spot markets",
        "List spot markets.",
    )
    .cli_aliases(&["ls"]);

    async fn run(inv: &Invocation, _args: &NoArgs) -> Result<(), anyhow::Error> {
        list(inv).await
    }
}

pub struct SpotGet;

impl CliCommand for SpotGet {
    type Args = SpotGetArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["spot", "get"],
        "market",
        "Get details for a specific spot pair",
        "Get details for a spot pair.",
    )
    .args(&[arg("pair")
        .kind(InputKind::MarketSymbol)
        .describe("Spot pair, for example PURR/USDC.")
        .resource_id("spot pair")]);

    async fn run(inv: &Invocation, args: &SpotGetArgs) -> Result<(), anyhow::Error> {
        get(inv, &args.pair).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_market() -> SpotMarketRow {
        SpotMarketRow {
            symbol: "PURR/USDC".to_string(),
            index: 10_000,
            base: "PURR".to_string(),
            quote: "USDC".to_string(),
            base_sz_decimals: 0,
            quote_sz_decimals: 6,
            tick_size_at_price_1: Some("0.000001".to_string()),
        }
    }

    #[test]
    fn spot_list_json_has_required_fields() {
        let output = SpotMarketsOutput::new(vec![sample_market()]);
        let json = output.to_json_value();
        let first = &json.as_array().unwrap()[0];

        assert_eq!(first["symbol"], "PURR/USDC");
        assert_eq!(first["base"], "PURR");
        assert_eq!(first["quote"], "USDC");
        assert_eq!(first["base_sz_decimals"], 0);
        assert_eq!(first["tick_size_at_price_1"], "0.000001");
        assert!(first.get("tick_size").is_none());
    }

    #[test]
    fn spot_list_rows_include_required_columns() {
        let output = SpotMarketsOutput::new(vec![sample_market()]);

        assert!(output.headers().contains(&"Symbol"));
        assert!(output.headers().contains(&"Base"));
        assert!(output.headers().contains(&"Quote"));
        assert!(output.headers().contains(&"Tick Size @ Price 1"));
        assert!(!output.headers().contains(&"Tick Size"));
        assert_eq!(output.rows()[0][0], "PURR/USDC");
    }

    #[test]
    fn spot_detail_json_is_single_object() {
        let output = SpotMarketDetailsOutput::new(sample_market());
        let json = output.to_json_value();

        assert!(json.is_object());
        assert_eq!(json["symbol"], "PURR/USDC");
        assert_eq!(json["quote_sz_decimals"], 6);
    }
}
