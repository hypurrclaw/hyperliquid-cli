use hyperliquid_cli::command_context::{
    CommandContext, CommandOutputContext, CommandTransportPolicy,
};
use hyperliquid_cli::commands::meta::{AssetContextRow, DexRow, MetaOutput, MetaSpotRow};
use hyperliquid_cli::output::{self, OutputFormat};
use serde_json::{Map, Value};

fn sample_meta_output() -> MetaOutput {
    MetaOutput::new(
        serde_json::json!({
            "universe": [
                {
                    "name": "BTC",
                    "maxLeverage": 50,
                    "szDecimals": 5,
                    "onlyIsolated": false
                }
            ]
        }),
        vec![serde_json::json!({
            "name": "BTC",
            "maxLeverage": 50,
            "szDecimals": 5,
            "onlyIsolated": false
        })],
        vec![MetaSpotRow {
            symbol: "PURR/USDC".to_string(),
            index: 10_000,
            base: "PURR".to_string(),
            quote: "USDC".to_string(),
            base_sz_decimals: 0,
        }],
        vec![AssetContextRow {
            name: "BTC".to_string(),
            fields: Map::from_iter([("midPx".to_string(), serde_json::json!("100"))]),
        }],
        vec![DexRow {
            name: "test-dex".to_string(),
            deployer_fee_scale: None,
        }],
    )
}

#[test]
fn meta_context_json_projection_does_not_leak_from_global_options() {
    output::set_json_options_with_limit(Some("dexes"), false, None);

    let context = CommandContext::new(
        "mainnet",
        "https://api.hyperliquid.xyz",
        CommandOutputContext::new(
            OutputFormat::Json,
            Some("universe,asset_contexts"),
            false,
            None,
        ),
        CommandTransportPolicy::CliProcess,
    );
    let rendered = context.render(&sample_meta_output());
    let json: Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json["universe"][0]["name"], "BTC");
    assert_eq!(json["asset_contexts"][0]["name"], "BTC");
    assert!(json.get("dexes").is_none());
    assert!(json.get("spot_universe").is_none());

    output::set_json_options_with_limit(None, false, None);
}
