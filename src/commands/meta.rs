//! Raw exchange metadata command.

use hypersdk::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::assets::{SpotListing, load_spot_markets};
use crate::command_spec::{CliCommand, CommandSpec, NoArgs};
use crate::errors::CliError;
use crate::http_api::ApiClient;
use crate::invocation::Invocation;
use crate::output::TableData;
use crate::output::format::thousands;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MetaSpotRow {
    pub symbol: String,
    pub index: usize,
    pub base: String,
    pub quote: String,
    pub base_sz_decimals: i64,
}

impl From<SpotListing> for MetaSpotRow {
    fn from(market: SpotListing) -> Self {
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

/// One `perpDexs` entry. hypersdk 0.3's `Dex` no longer carries the deployer fee
/// scale, so the CLI reads it from the raw response to keep the `meta` output stable.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPerpDex {
    name: String,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    deployer_fee_scale: Option<Decimal>,
}

impl From<RawPerpDex> for DexRow {
    fn from(dex: RawPerpDex) -> Self {
        Self {
            name: dex.name,
            deployer_fee_scale: dex.deployer_fee_scale,
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
                thousands(self.universe.len() as u64),
                self.universe
                    .iter()
                    .take(5)
                    .filter_map(asset_name_from_value)
                    .collect::<Vec<_>>()
                    .join(", "),
            ],
            vec![
                "spot_universe".to_string(),
                thousands(self.spot_universe.len() as u64),
                self.spot_universe
                    .iter()
                    .take(5)
                    .map(|market| market.symbol.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            ],
            vec![
                "asset_contexts".to_string(),
                thousands(self.asset_contexts.len() as u64),
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
                thousands(self.dexes.len() as u64),
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
    PerpDexs,
}

pub async fn show(inv: &Invocation) -> Result<(), anyhow::Error> {
    let api = inv.remote()?.api();
    let raw_meta_and_contexts = fetch_meta_and_asset_contexts(api).await?;
    let spot_universe = load_spot_universe(api).await?;
    let dexes = fetch_dexes(api).await?;
    inv.print(&raw_meta_and_contexts.into_output(spot_universe, dexes));
    Ok(())
}

async fn fetch_meta_and_asset_contexts(api: &ApiClient) -> Result<MetaAndAssetContexts, CliError> {
    api.info(
        &RawInfoRequest::MetaAndAssetCtxs,
        "loading raw exchange metadata",
    )
    .await
}

async fn fetch_dexes(api: &ApiClient) -> Result<Vec<DexRow>, CliError> {
    let dexes: Vec<Option<RawPerpDex>> = api
        .info(&RawInfoRequest::PerpDexs, "loading raw exchange metadata")
        .await?;
    Ok(dexes.into_iter().flatten().map(DexRow::from).collect())
}

async fn load_spot_universe(api: &ApiClient) -> Result<Vec<MetaSpotRow>, CliError> {
    Ok(load_spot_markets(api)
        .await?
        .into_iter()
        .map(MetaSpotRow::from)
        .collect())
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

pub struct Meta;

impl CliCommand for Meta {
    type Args = NoArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["meta"],
        "market",
        "Show raw exchange metadata",
        "Show raw exchange metadata.",
    );

    async fn run(inv: &Invocation, _args: &NoArgs) -> Result<(), anyhow::Error> {
        show(inv).await
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
