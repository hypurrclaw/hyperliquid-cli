/// Spot market commands.
///
/// Commands:
/// - `hyperliquid spot list` — list all spot markets
/// - `hyperliquid spot get <PAIR>` — get details for a specific spot pair
use clap::Args;
use hypersdk::Decimal;
use hypersdk::hypercore::{HttpClient, PriceTick, SpotMarket};
use serde::Serialize;
use std::time::{Duration, Instant};

use crate::command_context::CommandContext;
use crate::commands::{
    AssetResolver, RawSpotMarket, ResolvedAsset, load_raw_spot_markets, map_api_error,
};
use crate::errors::CliError;
use crate::output::{OutputFormat, TableData};

#[derive(Args, Debug)]
pub struct SpotListArgs {}

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

impl From<SpotMarket> for SpotMarketRow {
    fn from(market: SpotMarket) -> Self {
        let base = market.base().clone();
        let quote = market.quote().clone();
        Self {
            symbol: market.symbol(),
            index: market.index,
            base: base.name,
            quote: quote.name,
            base_sz_decimals: base.sz_decimals,
            quote_sz_decimals: quote.sz_decimals,
            tick_size_at_price_1: market
                .tick_for(Decimal::from(1))
                .map(|tick| tick.to_string()),
        }
    }
}

impl From<RawSpotMarket> for SpotMarketRow {
    fn from(market: RawSpotMarket) -> Self {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotMarketsResult {
    pub output: SpotMarketsOutput,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotMarketDetailsResult {
    pub output: SpotMarketDetailsOutput,
    pub elapsed: Duration,
}

/// Fetch all spot markets.
pub async fn list_query(client: &HttpClient) -> Result<SpotMarketsResult, anyhow::Error> {
    let start = Instant::now();
    let markets = load_spot_market_rows(client).await?;
    let output = SpotMarketsOutput::new(markets);

    Ok(SpotMarketsResult {
        output,
        elapsed: start.elapsed(),
    })
}

/// Fetch and render all spot markets through a per-call output context.
pub async fn list_with_context(context: &CommandContext<'_>) -> Result<(), anyhow::Error> {
    let client = context
        .hypercore_client()
        .ok_or_else(|| anyhow::anyhow!("spot list command requires a Hyperliquid HTTP client"))?;
    let result = list_query(client).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

/// Fetch and render all spot markets.
pub async fn list(client: &HttpClient, format: OutputFormat) -> Result<(), anyhow::Error> {
    let result = list_query(client).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Resolve, fetch, and render one spot market.
pub async fn get(
    client: &HttpClient,
    resolver: &AssetResolver,
    pair: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = get_query(client, resolver, pair).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Resolve and fetch one spot market.
pub async fn get_query(
    client: &HttpClient,
    resolver: &AssetResolver,
    pair: &str,
) -> Result<SpotMarketDetailsResult, anyhow::Error> {
    let start = Instant::now();
    let resolved = resolver.resolve_spot(pair)?;
    let (resolved_symbol, resolved_index) = match resolved {
        ResolvedAsset::Spot { symbol, index, .. } => (symbol, index),
        ResolvedAsset::Perp { .. } => {
            return Err(CliError::AssetNotFoundNoSuggestion {
                asset: pair.to_string(),
            }
            .into());
        }
    };

    let market = load_spot_market_rows(client)
        .await?
        .into_iter()
        .find(|market| {
            market.index == resolved_index || market.symbol.eq_ignore_ascii_case(&resolved_symbol)
        })
        .ok_or_else(|| CliError::AssetNotFoundNoSuggestion {
            asset: pair.to_string(),
        })?;

    Ok(SpotMarketDetailsResult {
        output: SpotMarketDetailsOutput::new(market),
        elapsed: start.elapsed(),
    })
}

async fn load_spot_market_rows(client: &HttpClient) -> Result<Vec<SpotMarketRow>, CliError> {
    match client.spot().await {
        Ok(markets) => Ok(markets.into_iter().map(SpotMarketRow::from).collect()),
        Err(spot_err) => load_raw_spot_markets(client)
            .await
            .map(|markets| markets.into_iter().map(SpotMarketRow::from).collect())
            .map_err(|_| map_api_error(spot_err)),
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
