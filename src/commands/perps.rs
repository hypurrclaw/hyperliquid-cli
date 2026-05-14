/// Perpetual market commands.
///
/// Commands:
/// - `hyperliquid perps list` — list all perpetual markets
/// - `hyperliquid perps get <COIN>` — get details for a specific perpetual market
use clap::Args;
use hypersdk::Decimal;
use hypersdk::hypercore::{HttpClient, PerpMarket};
use serde::Serialize;
use std::time::{Duration, Instant};

use crate::command_context::CommandContext;
use crate::commands::{AssetQuery, AssetResolver, ResolvedAsset, map_api_error, parse_asset_query};
use crate::errors::CliError;
use crate::output::{OutputFormat, TableData};

#[derive(Args, Debug)]
pub struct PerpsListArgs {
    /// Query markets from a specific HIP-3 DEX
    #[arg(long)]
    pub dex: Option<String>,
}

#[derive(Args, Debug)]
pub struct PerpsGetArgs {
    /// Coin name (e.g., BTC, ETH, SOL)
    pub coin: String,

    /// Query a market from a specific HIP-3 DEX
    #[arg(long)]
    pub dex: Option<String>,
}

/// Renderable perpetual market row.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PerpMarketRow {
    pub name: String,
    pub index: usize,
    pub max_leverage: u64,
    pub sz_decimals: i64,
    pub collateral: String,
    pub isolated_margin: bool,
    pub tick_size_at_price_1: Option<String>,
}

impl From<PerpMarket> for PerpMarketRow {
    fn from(market: PerpMarket) -> Self {
        let tick_size_at_price_1 = market
            .tick_for(Decimal::from(1))
            .map(|tick| tick.to_string());
        Self {
            name: market.name,
            index: market.index,
            max_leverage: market.max_leverage,
            sz_decimals: market.sz_decimals,
            collateral: market.collateral.name,
            isolated_margin: market.isolated_margin,
            tick_size_at_price_1,
        }
    }
}

/// Output wrapper for perpetual markets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerpMarketsOutput {
    markets: Vec<PerpMarketRow>,
}

impl PerpMarketsOutput {
    #[must_use]
    pub fn new(markets: Vec<PerpMarketRow>) -> Self {
        Self { markets }
    }
}

impl TableData for PerpMarketsOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Name",
            "Index",
            "Max Leverage",
            "Sz Decimals",
            "Collateral",
            "Isolated",
            "Tick Size @ Price 1",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.markets
            .iter()
            .map(|market| {
                vec![
                    market.name.clone(),
                    market.index.to_string(),
                    format!("{}x", market.max_leverage),
                    market.sz_decimals.to_string(),
                    market.collateral.clone(),
                    market.isolated_margin.to_string(),
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

/// Output wrapper for a single perpetual market.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerpMarketDetailsOutput {
    market: PerpMarketRow,
}

impl PerpMarketDetailsOutput {
    #[must_use]
    pub fn new(market: PerpMarketRow) -> Self {
        Self { market }
    }
}

impl TableData for PerpMarketDetailsOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Name",
            "Index",
            "Max Leverage",
            "Sz Decimals",
            "Collateral",
            "Isolated",
            "Tick Size @ Price 1",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        PerpMarketsOutput::new(vec![self.market.clone()]).rows()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.market).unwrap_or_else(|_| serde_json::json!({}))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerpMarketsResult {
    pub output: PerpMarketsOutput,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerpMarketDetailsResult {
    pub output: PerpMarketDetailsOutput,
    pub elapsed: Duration,
}

/// Fetch perpetual markets, optionally from a HIP-3 DEX.
pub async fn list_query(
    client: &HttpClient,
    args: &PerpsListArgs,
) -> Result<PerpMarketsResult, anyhow::Error> {
    let start = Instant::now();
    let markets = if let Some(dex_name) = args.dex.as_deref() {
        let dex = find_dex(client, dex_name).await?;
        client.perps_from(dex).await.map_err(map_api_error)?
    } else {
        client.perps().await.map_err(map_api_error)?
    };
    let markets = markets.into_iter().map(PerpMarketRow::from).collect();
    let output = PerpMarketsOutput::new(markets);

    Ok(PerpMarketsResult {
        output,
        elapsed: start.elapsed(),
    })
}

/// Fetch and render perpetual markets through a per-call output context.
pub async fn list_with_context(
    context: &CommandContext<'_>,
    args: &PerpsListArgs,
) -> Result<(), anyhow::Error> {
    let client = context
        .hypercore_client()
        .ok_or_else(|| anyhow::anyhow!("perps list command requires a Hyperliquid HTTP client"))?;
    let result = list_query(client, args).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

/// Fetch and render perpetual markets, optionally from a HIP-3 DEX.
pub async fn list(
    client: &HttpClient,
    args: &PerpsListArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = list_query(client, args).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Resolve, fetch, and render one perpetual market.
pub async fn get(
    client: &HttpClient,
    resolver: &AssetResolver,
    coin: &str,
    dex: Option<&str>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = get_query(client, resolver, coin, dex).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Resolve and fetch one perpetual market.
pub async fn get_query(
    client: &HttpClient,
    resolver: &AssetResolver,
    coin: &str,
    dex: Option<&str>,
) -> Result<PerpMarketDetailsResult, anyhow::Error> {
    let start = Instant::now();
    let query = qualify_dex_asset(dex, coin);
    let market = if let AssetQuery::Hip3 {
        dex: dex_name,
        token,
    } = parse_asset_query(&query)
    {
        let dex = find_dex(client, &dex_name).await?;
        let prefixed = format!("{dex_name}:");
        client
            .perps_from(dex)
            .await
            .map_err(map_api_error)?
            .into_iter()
            .find(|market| {
                let display_token = market.name.strip_prefix(&prefixed).unwrap_or(&market.name);
                display_token.eq_ignore_ascii_case(&token)
                    || market.name.eq_ignore_ascii_case(&token)
            })
            .map(PerpMarketRow::from)
            .ok_or_else(|| CliError::AssetNotFoundNoSuggestion {
                asset: query.clone(),
            })?
    } else {
        let resolved = resolver.resolve_perp(&query)?;
        let (resolved_name, resolved_index, resolved_dex) = match resolved {
            ResolvedAsset::Perp {
                name, index, dex, ..
            } => (name, index, dex),
            ResolvedAsset::Spot { .. } => {
                return Err(CliError::AssetNotFoundNoSuggestion {
                    asset: coin.to_string(),
                }
                .into());
            }
        };

        let markets = if let Some(dex_name) = resolved_dex.as_deref() {
            let dex = find_dex(client, dex_name).await?;
            client.perps_from(dex).await.map_err(map_api_error)?
        } else {
            client.perps().await.map_err(map_api_error)?
        };

        markets
            .into_iter()
            .find(|market| {
                market.index == resolved_index || market.name.eq_ignore_ascii_case(&resolved_name)
            })
            .map(PerpMarketRow::from)
            .ok_or_else(|| CliError::AssetNotFoundNoSuggestion {
                asset: query.clone(),
            })?
    };

    Ok(PerpMarketDetailsResult {
        output: PerpMarketDetailsOutput::new(market),
        elapsed: start.elapsed(),
    })
}

async fn find_dex(
    client: &HttpClient,
    dex_name: &str,
) -> Result<hypersdk::hypercore::Dex, CliError> {
    client
        .perp_dexs()
        .await
        .map_err(map_api_error)?
        .into_iter()
        .find(|dex| dex.name().eq_ignore_ascii_case(dex_name))
        .ok_or_else(|| CliError::Unsupported(format!("Unknown DEX: {dex_name}")))
}

fn qualify_dex_asset(dex: Option<&str>, coin: &str) -> String {
    match dex {
        Some(dex) if !coin.contains(':') => format!("{dex}:{coin}"),
        _ => coin.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_market() -> PerpMarketRow {
        PerpMarketRow {
            name: "BTC".to_string(),
            index: 0,
            max_leverage: 50,
            sz_decimals: 5,
            collateral: "USDC".to_string(),
            isolated_margin: false,
            tick_size_at_price_1: Some("1".to_string()),
        }
    }

    #[test]
    fn perps_list_json_has_required_fields() {
        let output = PerpMarketsOutput::new(vec![sample_market()]);
        let json = output.to_json_value();
        let first = &json.as_array().unwrap()[0];

        assert_eq!(first["name"], "BTC");
        assert_eq!(first["max_leverage"], 50);
        assert_eq!(first["sz_decimals"], 5);
        assert_eq!(first["tick_size_at_price_1"], "1");
        assert!(first.get("tick_size").is_none());
    }

    #[test]
    fn perps_list_rows_include_required_columns() {
        let output = PerpMarketsOutput::new(vec![sample_market()]);

        assert!(output.headers().contains(&"Name"));
        assert!(output.headers().contains(&"Max Leverage"));
        assert!(output.headers().contains(&"Sz Decimals"));
        assert!(output.headers().contains(&"Tick Size @ Price 1"));
        assert!(!output.headers().contains(&"Tick Size"));
        assert_eq!(output.rows()[0][0], "BTC");
        assert_eq!(output.rows()[0][2], "50x");
    }

    #[test]
    fn perps_detail_json_is_single_object() {
        let output = PerpMarketDetailsOutput::new(sample_market());
        let json = output.to_json_value();

        assert!(json.is_object());
        assert_eq!(json["name"], "BTC");
        assert_eq!(json["collateral"], "USDC");
    }
}
