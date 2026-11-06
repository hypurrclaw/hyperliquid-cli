use hyperliquid_cli::commands::account::{AbstractionOutput, AbstractionRow, FillRow, FillsOutput};
use hyperliquid_cli::commands::meta::{AssetContextRow, DexRow, MetaOutput, MetaSpotRow};
use hyperliquid_cli::commands::orderbook::{
    BookLevelRow, BookSnapshot, CandleRow, CandlesOutput, FundingRow, MidRow, MidsOutput, SpreadRow,
};
use hyperliquid_cli::commands::perps::{PerpMarketRow, PerpMarketsOutput};
use hyperliquid_cli::commands::spot::{SpotMarketRow, SpotMarketsOutput};
use hyperliquid_cli::commands::status::StatusOutput;
use hyperliquid_cli::output::terminal::OutputEnv;
use hyperliquid_cli::output::{JsonRenderOptions, OutputFormat, Session};
use rust_decimal::Decimal;
use serde_json::{Map, Value};
use std::str::FromStr;

fn json_session(select: Option<&str>) -> Session {
    json_session_with_limit(select, None)
}

fn json_session_with_limit(select: Option<&str>, max_results: Option<usize>) -> Session {
    Session::new(
        OutputFormat::Json,
        JsonRenderOptions::from_cli(select, false, max_results),
        OutputEnv::default(),
    )
}

fn dec(input: &str) -> Decimal {
    Decimal::from_str(input).unwrap()
}

fn sample_status_output() -> StatusOutput {
    StatusOutput {
        network: "mainnet".to_string(),
        api_url: "https://api.hyperliquid.xyz".to_string(),
        health: "healthy".to_string(),
        latency_ms: 42,
        perps_count: 100,
        mids_count: 100,
        rate_limit_status: "ok".to_string(),
        rate_limit_note: "no rate-limit response observed".to_string(),
    }
}

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
fn account_fills_projection_selects_requested_fields() {
    let output = FillsOutput::new(vec![FillRow {
        coin: "BTC".to_string(),
        side: "buy".to_string(),
        price: Decimal::from(50_000),
        size: Decimal::new(1, 3),
        direction: "Open Long".to_string(),
        closed_pnl: Decimal::ZERO,
        fee: Decimal::new(10, 2),
        fee_token: "USDC".to_string(),
        oid: 42,
        time: 1_700_000_000_000,
        liquidity: "taker".to_string(),
        trade_id: 7,
        cloid: None,
    }]);
    let rendered = json_session(Some("coin,time")).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json[0]["coin"], "BTC");
    assert_eq!(json[0]["time"], 1_700_000_000_000_u64);
    assert!(json[0].get("fee").is_none());
}

#[test]
fn account_abstraction_projection_selects_requested_fields() {
    let output = AbstractionOutput {
        row: AbstractionRow {
            user: "0x0000000000000000000000000000000000000001".to_string(),
            raw_mode: "unifiedAccount".to_string(),
            normalized_mode: "unified-account".to_string(),
        },
    };
    let rendered = json_session(Some("user,normalized_mode")).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json["user"], "0x0000000000000000000000000000000000000001");
    assert_eq!(json["normalized_mode"], "unified-account");
    assert!(json.get("raw_mode").is_none());
}

#[test]
fn status_projection_selects_requested_fields() {
    let rendered = json_session(Some("network,latency_ms")).render(&sample_status_output());
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json["network"], "mainnet");
    assert_eq!(json["latency_ms"], 42);
    assert!(json.get("health").is_none());
}

#[test]
fn meta_projection_selects_requested_fields() {
    let rendered = json_session(Some("universe,asset_contexts")).render(&sample_meta_output());
    let json: Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json["universe"][0]["name"], "BTC");
    assert_eq!(json["asset_contexts"][0]["name"], "BTC");
    assert!(json.get("dexes").is_none());
    assert!(json.get("spot_universe").is_none());
}

#[test]
fn perps_list_projection_selects_requested_fields() {
    let output = PerpMarketsOutput::new(vec![PerpMarketRow {
        name: "BTC".to_string(),
        index: 0,
        max_leverage: 50,
        sz_decimals: 5,
        collateral: "USDC".to_string(),
        isolated_margin: false,
        tick_size_at_price_1: Some("1".to_string()),
    }]);
    let rendered = json_session(Some("name,index")).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json[0]["name"], "BTC");
    assert_eq!(json[0]["index"], 0);
    assert!(json[0].get("collateral").is_none());
}

#[test]
fn spot_list_projection_selects_requested_fields() {
    let output = SpotMarketsOutput::new(vec![SpotMarketRow {
        symbol: "PURR/USDC".to_string(),
        index: 10_000,
        base: "PURR".to_string(),
        quote: "USDC".to_string(),
        base_sz_decimals: 0,
        quote_sz_decimals: 6,
        tick_size_at_price_1: Some("0.000001".to_string()),
    }]);
    let rendered = json_session(Some("symbol,base")).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json[0]["symbol"], "PURR/USDC");
    assert_eq!(json[0]["base"], "PURR");
    assert!(json[0].get("quote").is_none());
}

#[test]
fn book_projection_selects_requested_fields() {
    let output = BookSnapshot {
        coin: "BTC".to_string(),
        time: 123,
        bids: vec![BookLevelRow {
            price: dec("100"),
            size: dec("1"),
            depth: dec("1"),
            orders: 2,
        }],
        asks: vec![BookLevelRow {
            price: dec("101"),
            size: dec("1.5"),
            depth: dec("1.5"),
            orders: 3,
        }],
    };
    let rendered = json_session(Some("coin,bids")).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json["coin"], "BTC");
    assert!(json["bids"].is_array());
    assert!(json.get("asks").is_none());
}

#[test]
fn mids_projection_preserves_select_aliases() {
    let output = MidsOutput::new(
        vec![MidRow {
            name: "BTC".to_string(),
            mid: dec("100"),
        }],
        Some(&["coin".to_string(), "price".to_string()]),
    );
    let rendered = json_session(Some("coin,price")).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json[0]["coin"], "BTC");
    assert_eq!(json[0]["price"], "100");
    assert!(json[0].get("mid").is_none());
}

#[test]
fn candles_projection_applies_result_limit() {
    let output = CandlesOutput::new(vec![
        CandleRow {
            timestamp: 1,
            close_time: 2,
            coin: "BTC".to_string(),
            interval: "15m".to_string(),
            open: dec("100"),
            high: dec("110"),
            low: dec("90"),
            close: dec("105"),
            volume: dec("42"),
            num_trades: 7,
        },
        CandleRow {
            timestamp: 3,
            close_time: 4,
            coin: "BTC".to_string(),
            interval: "15m".to_string(),
            open: dec("106"),
            high: dec("112"),
            low: dec("100"),
            close: dec("111"),
            volume: dec("21"),
            num_trades: 8,
        },
    ]);
    let rendered = json_session_with_limit(None, Some(1)).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_eq!(json[0]["timestamp"], 1);
}

#[test]
fn spread_and_funding_projection_select_requested_fields() {
    let spread = SpreadRow {
        coin: "BTC".to_string(),
        bid: dec("100"),
        ask: dec("101"),
        spread: dec("1"),
        spread_pct: dec("1"),
    };
    let spread_rendered = json_session(Some("coin,spread_pct")).render(&spread);
    let spread_json: serde_json::Value = serde_json::from_str(&spread_rendered).unwrap();
    assert_eq!(spread_json["coin"], "BTC");
    assert_eq!(spread_json["spread_pct"], "1");
    assert!(spread_json.get("spread").is_none());

    let funding = FundingRow {
        coin: "BTC".to_string(),
        current_funding_rate: Some(dec("0.0001")),
        predicted_funding_rate: dec("0.0002"),
        premium: Some(dec("0.00005")),
        mark_price: Some(dec("100")),
        oracle_price: Some(dec("99")),
        last_funding_time: Some(123),
    };
    let funding_rendered = json_session(Some("coin,predicted_funding_rate")).render(&funding);
    let funding_json: serde_json::Value = serde_json::from_str(&funding_rendered).unwrap();
    assert_eq!(funding_json["coin"], "BTC");
    assert_eq!(funding_json["predicted_funding_rate"], "0.0002");
    assert!(funding_json.get("current_funding_rate").is_none());
}
