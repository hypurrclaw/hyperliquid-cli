/// Perpetual market commands.
///
/// Commands:
/// - `hyperliquid perps list` — list all perpetual markets
/// - `hyperliquid perps get <COIN>` — get details for a specific perpetual market
use clap::Args;
use hypersdk::Decimal;
use hypersdk::hypercore::PerpMarket;
use serde::Serialize;

use crate::assets::{
    AssetQuery, find_perp_dex, load_hip3_market, parse_asset_query, qualify_dex_asset,
};
use crate::errors::CliError;
use crate::errors::from_sdk;
use crate::invocation::Invocation;
use crate::output::TableData;

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

    fn empty_message(&self) -> Option<&str> {
        Some("No perpetual markets.")
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

/// Fetch and render perpetual markets, optionally from a HIP-3 DEX.
pub async fn list(inv: &Invocation, args: &PerpsListArgs) -> Result<(), anyhow::Error> {
    let client = inv.remote()?.client();
    let markets = if let Some(dex_name) = args.dex.as_deref() {
        let dex = find_perp_dex(client, dex_name).await?;
        client.perps_from(dex).await.map_err(from_sdk)?
    } else {
        client.perps().await.map_err(from_sdk)?
    };
    let markets = markets.into_iter().map(PerpMarketRow::from).collect();
    inv.print(&PerpMarketsOutput::new(markets));
    Ok(())
}

/// Resolve, fetch, and render one perpetual market.
pub async fn get(inv: &Invocation, args: &PerpsGetArgs) -> Result<(), anyhow::Error> {
    let client = inv.remote()?.client();
    let resolver = inv.asset_resolver().await?;
    let query = qualify_dex_asset(args.dex.as_deref(), &args.coin);
    let market = if let AssetQuery::Hip3 {
        dex: dex_name,
        token,
    } = parse_asset_query(&query)
    {
        load_hip3_market(client, &dex_name, &token)
            .await?
            .map(|(_, market)| PerpMarketRow::from(market))
            .ok_or_else(|| CliError::AssetNotFoundNoSuggestion {
                asset: query.clone(),
            })?
    } else {
        let resolved = resolver.resolve_perp(&query).map_err(CliError::from)?;
        client
            .perps()
            .await
            .map_err(from_sdk)?
            .into_iter()
            .find(|market| {
                market.index == resolved.index || market.name.eq_ignore_ascii_case(&resolved.name)
            })
            .map(PerpMarketRow::from)
            .ok_or_else(|| CliError::AssetNotFoundNoSuggestion {
                asset: query.clone(),
            })?
    };

    inv.print(&PerpMarketDetailsOutput::new(market));
    Ok(())
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
