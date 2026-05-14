//! Raw exchange metadata command.

use std::time::{Duration, Instant};

use hypersdk::Decimal;
use hypersdk::hypercore::{Dex, HttpClient, SpotMarket};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::command_context::CommandContext;
use crate::commands::{RawSpotMarket, load_raw_spot_markets, map_api_error, raw_info_base_url};
use crate::errors::CliError;
use crate::output::{OutputFormat, TableData};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MetaSpotRow {
    pub symbol: String,
    pub index: usize,
    pub base: String,
    pub quote: String,
    pub base_sz_decimals: i64,
}

impl From<SpotMarket> for MetaSpotRow {
    fn from(market: SpotMarket) -> Self {
        Self {
            symbol: market.symbol(),
            index: market.index,
            base: market.base().name.clone(),
            quote: market.quote().name.clone(),
            base_sz_decimals: market.base().sz_decimals,
        }
    }
}

impl From<RawSpotMarket> for MetaSpotRow {
    fn from(market: RawSpotMarket) -> Self {
        Self {
            symbol: market.symbol,
            index: market.index,
            base: market.base,
            quote: market.quote,
            base_sz_decimals: market.base_sz_decimals,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AssetContextRow {
    pub name: String,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

impl AssetContextRow {
    fn mid_for_summary(&self) -> Option<String> {
        ["midPx", "mid", "markPx"]
            .into_iter()
            .find_map(|field| value_to_summary(self.fields.get(field)?))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DexRow {
    pub name: String,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub deployer_fee_scale: Option<Decimal>,
}

impl From<Dex> for DexRow {
    fn from(dex: Dex) -> Self {
        Self {
            name: dex.name().to_string(),
            deployer_fee_scale: dex.deployer_fee_scale(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetaOutput {
    raw_meta: Value,
    universe: Vec<Value>,
    spot_universe: Vec<MetaSpotRow>,
    asset_contexts: Vec<AssetContextRow>,
    dexes: Vec<DexRow>,
}

impl MetaOutput {
    #[must_use]
    pub fn new(
        raw_meta: Value,
        universe: Vec<Value>,
        spot_universe: Vec<MetaSpotRow>,
        asset_contexts: Vec<AssetContextRow>,
        dexes: Vec<DexRow>,
    ) -> Self {
        Self {
            raw_meta,
            universe,
            spot_universe,
            asset_contexts,
            dexes,
        }
    }
}

impl TableData for MetaOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Section", "Count", "Sample"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![
            vec![
                "universe".to_string(),
                self.universe.len().to_string(),
                self.universe
                    .iter()
                    .take(5)
                    .filter_map(asset_name_from_value)
                    .collect::<Vec<_>>()
                    .join(", "),
            ],
            vec![
                "spot_universe".to_string(),
                self.spot_universe.len().to_string(),
                self.spot_universe
                    .iter()
                    .take(5)
                    .map(|market| market.symbol.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            ],
            vec![
                "asset_contexts".to_string(),
                self.asset_contexts.len().to_string(),
                self.asset_contexts
                    .iter()
                    .take(5)
                    .map(|ctx| {
                        let mid = ctx.mid_for_summary().unwrap_or_else(|| "n/a".to_string());
                        format!("{}={mid}", ctx.name)
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ],
            vec![
                "dexes".to_string(),
                self.dexes.len().to_string(),
                self.dexes
                    .iter()
                    .take(5)
                    .map(|dex| dex.name.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            ],
        ]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "meta": self.raw_meta,
            "universe": self.universe,
            "spot_universe": self.spot_universe,
            "asset_contexts": self.asset_contexts,
            "dexes": self.dexes,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetaResult {
    pub output: MetaOutput,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct MetaAndAssetContexts(Value, Vec<Map<String, Value>>);

impl MetaAndAssetContexts {
    fn into_output(self, spot_universe: Vec<MetaSpotRow>, dexes: Vec<DexRow>) -> MetaOutput {
        let raw_meta = self.0;
        let universe = raw_meta
            .get("universe")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let asset_contexts = self
            .1
            .into_iter()
            .enumerate()
            .map(|(index, fields)| {
                let name = universe
                    .get(index)
                    .and_then(asset_name_from_value)
                    .unwrap_or_else(|| index.to_string());
                AssetContextRow { name, fields }
            })
            .collect();

        MetaOutput::new(raw_meta, universe, spot_universe, asset_contexts, dexes)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
enum RawInfoRequest {
    MetaAndAssetCtxs,
}

/// Fetch exchange metadata: perpetual universe, spot universe, asset contexts, and DEXes.
pub async fn query(client: &HttpClient) -> Result<MetaResult, anyhow::Error> {
    let start = Instant::now();
    let raw_meta_and_contexts = fetch_meta_and_asset_contexts(client).await?;
    let spot_universe = load_spot_universe(client).await?;
    let dexes = client
        .perp_dexs()
        .await
        .map_err(map_api_error)?
        .into_iter()
        .map(DexRow::from)
        .collect();
    let output = raw_meta_and_contexts.into_output(spot_universe, dexes);

    Ok(MetaResult {
        output,
        elapsed: start.elapsed(),
    })
}

/// Fetch and render exchange metadata through a per-call output context.
pub async fn show_with_context(context: &CommandContext<'_>) -> Result<(), anyhow::Error> {
    let client = context
        .hypercore_client()
        .ok_or_else(|| anyhow::anyhow!("meta command requires a Hyperliquid HTTP client"))?;
    let result = query(client).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

/// Fetch and render exchange metadata: perpetual universe, spot universe, asset contexts, and DEXes.
pub async fn show(client: &HttpClient, format: OutputFormat) -> Result<(), anyhow::Error> {
    let result = query(client).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

async fn fetch_meta_and_asset_contexts(
    client: &HttpClient,
) -> Result<MetaAndAssetContexts, CliError> {
    let mut api_url = raw_info_base_url(client)?;
    api_url.set_path("/info");

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .tcp_nodelay(true)
        .build()
        .map_err(|err| CliError::Internal(anyhow::Error::new(err)))?;
    let response = http_client
        .post(api_url)
        .json(&RawInfoRequest::MetaAndAssetCtxs)
        .send()
        .await
        .map_err(|err| map_api_error(anyhow::Error::new(err)))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| map_api_error(anyhow::Error::new(err)))?;

    if !status.is_success() {
        return Err(map_api_error(anyhow::anyhow!("HTTP {status} body={text}")));
    }

    serde_json::from_str(&text).map_err(|err| {
        CliError::Internal(anyhow::anyhow!(
            "decode failed while loading raw exchange metadata: {err}; body={text}"
        ))
    })
}

async fn load_spot_universe(client: &HttpClient) -> Result<Vec<MetaSpotRow>, CliError> {
    match client.spot().await {
        Ok(markets) => Ok(markets.into_iter().map(MetaSpotRow::from).collect()),
        Err(spot_err) => load_raw_spot_markets(client)
            .await
            .map(|markets| markets.into_iter().map(MetaSpotRow::from).collect())
            .map_err(|_| map_api_error(spot_err)),
    }
}

fn asset_name_from_value(value: &Value) -> Option<String> {
    value
        .get("name")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn value_to_summary(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_json_contains_universe_and_asset_contexts() {
        let output = MetaOutput::new(
            serde_json::json!({
                "universe": [{
                    "name": "BTC",
                    "maxLeverage": 50,
                    "szDecimals": 5,
                    "onlyIsolated": false
                }]
            }),
            vec![serde_json::json!({
                "name": "BTC",
                "maxLeverage": 50,
                "szDecimals": 5,
                "onlyIsolated": false
            })],
            vec![],
            vec![AssetContextRow {
                name: "BTC".to_string(),
                fields: Map::from_iter([
                    ("funding".to_string(), serde_json::json!("0.0001")),
                    ("openInterest".to_string(), serde_json::json!("42.5")),
                    ("markPx".to_string(), serde_json::json!("101")),
                    ("oraclePx".to_string(), serde_json::json!("100.5")),
                    ("midPx".to_string(), serde_json::json!("100")),
                    ("prevDayPx".to_string(), serde_json::json!("99")),
                    ("dayNtlVlm".to_string(), serde_json::json!("123456.7")),
                ]),
            }],
            vec![],
        );
        let json = output.to_json_value();

        assert_eq!(json["universe"][0]["name"], "BTC");
        assert_eq!(json["asset_contexts"][0]["midPx"], "100");
        assert_eq!(json["asset_contexts"][0]["funding"], "0.0001");
        assert_eq!(json["asset_contexts"][0]["openInterest"], "42.5");
        assert_eq!(json["asset_contexts"][0]["markPx"], "101");
        assert_eq!(json["asset_contexts"][0]["oraclePx"], "100.5");
        assert_eq!(json["asset_contexts"][0]["prevDayPx"], "99");
        assert_eq!(json["asset_contexts"][0]["dayNtlVlm"], "123456.7");
    }

    #[test]
    fn meta_output_from_raw_response_pairs_names_and_preserves_raw_context_fields() {
        let raw: MetaAndAssetContexts = serde_json::from_value(serde_json::json!([
            {
                "universe": [
                    {
                        "name": "BTC",
                        "szDecimals": 5,
                        "maxLeverage": 50,
                        "onlyIsolated": false
                    }
                ],
                "marginTables": [[50, {"lowerBound": "0.0", "maxLeverage": 50}]]
            },
            [
                {
                    "dayNtlVlm": "123456.7",
                    "funding": "0.0001",
                    "impactPxs": ["99.5", "100.5"],
                    "markPx": "101",
                    "midPx": "100",
                    "openInterest": "42.5",
                    "oraclePx": "100.5",
                    "premium": "0.00002",
                    "prevDayPx": "99"
                }
            ]
        ]))
        .unwrap();

        let output = raw.into_output(vec![], vec![]);
        let json = output.to_json_value();

        assert_eq!(json["meta"]["marginTables"][0][0], 50);
        assert_eq!(json["asset_contexts"][0]["name"], "BTC");
        assert_eq!(json["asset_contexts"][0]["funding"], "0.0001");
        assert_eq!(json["asset_contexts"][0]["openInterest"], "42.5");
        assert_eq!(json["asset_contexts"][0]["impactPxs"][0], "99.5");
    }

    #[test]
    fn meta_table_summarizes_metadata_sections() {
        let output = MetaOutput::new(Value::Null, vec![], vec![], vec![], vec![]);
        let rows = output.rows();

        assert_eq!(rows[0][0], "universe");
        assert_eq!(rows[1][0], "spot_universe");
        assert_eq!(rows[2][0], "asset_contexts");
    }
}
