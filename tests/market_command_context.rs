use hyperliquid_cli::command_context::{
    CommandContext, CommandOutputContext, CommandTransportPolicy,
};
use hyperliquid_cli::commands::orderbook::{
    BookLevelRow, BookSnapshot, CandleRow, CandlesOutput, FundingRow, MidRow, MidsOutput, SpreadRow,
};
use hyperliquid_cli::commands::perps::{PerpMarketRow, PerpMarketsOutput};
use hyperliquid_cli::commands::spot::{SpotMarketRow, SpotMarketsOutput};
use hyperliquid_cli::output::{self, OutputFormat};
use rust_decimal::Decimal;
use std::str::FromStr;

fn json_context(select: Option<&str>) -> CommandContext<'static> {
    json_context_with_limit(select, None)
}

fn json_context_with_limit(
    select: Option<&str>,
    max_results: Option<usize>,
) -> CommandContext<'static> {
    CommandContext::new(
        "mainnet",
        "https://api.hyperliquid.xyz",
        CommandOutputContext::new(OutputFormat::Json, select, false, max_results),
        CommandTransportPolicy::CliProcess,
    )
}

fn dec(input: &str) -> Decimal {
    Decimal::from_str(input).unwrap()
}

#[test]
fn perps_list_context_json_projection_does_not_leak_from_global_options() {
    output::set_json_options_with_limit(Some("collateral"), false, None);

    let output = PerpMarketsOutput::new(vec![PerpMarketRow {
        name: "BTC".to_string(),
        index: 0,
        max_leverage: 50,
        sz_decimals: 5,
        collateral: "USDC".to_string(),
        isolated_margin: false,
        tick_size_at_price_1: Some("1".to_string()),
    }]);
    let rendered = json_context(Some("name,index")).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json[0]["name"], "BTC");
    assert_eq!(json[0]["index"], 0);
    assert!(json[0].get("collateral").is_none());

    output::set_json_options_with_limit(None, false, None);
}

#[test]
fn spot_list_context_json_projection_does_not_leak_from_global_options() {
    output::set_json_options_with_limit(Some("quote"), false, None);

    let output = SpotMarketsOutput::new(vec![SpotMarketRow {
        symbol: "PURR/USDC".to_string(),
        index: 10_000,
        base: "PURR".to_string(),
        quote: "USDC".to_string(),
        base_sz_decimals: 0,
        quote_sz_decimals: 6,
        tick_size_at_price_1: Some("0.000001".to_string()),
    }]);
    let rendered = json_context(Some("symbol,base")).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json[0]["symbol"], "PURR/USDC");
    assert_eq!(json[0]["base"], "PURR");
    assert!(json[0].get("quote").is_none());

    output::set_json_options_with_limit(None, false, None);
}

#[test]
fn book_context_json_projection_does_not_leak_from_global_options() {
    output::set_json_options_with_limit(Some("asks"), false, None);

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
    let rendered = json_context(Some("coin,bids")).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json["coin"], "BTC");
    assert!(json["bids"].is_array());
    assert!(json.get("asks").is_none());

    output::set_json_options_with_limit(None, false, None);
}

#[test]
fn mids_context_preserves_select_aliases_without_global_projection() {
    output::set_json_options_with_limit(Some("ETH"), false, None);

    let output = MidsOutput::new(
        vec![MidRow {
            name: "BTC".to_string(),
            mid: dec("100"),
        }],
        Some("coin,price"),
    );
    let rendered = json_context(Some("coin,price")).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json[0]["coin"], "BTC");
    assert_eq!(json[0]["price"], "100");
    assert!(json[0].get("mid").is_none());

    output::set_json_options_with_limit(None, false, None);
}

#[test]
fn orderbook_context_applies_explicit_result_limit() {
    output::set_json_options_with_limit(None, false, Some(3));

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
    let rendered = json_context_with_limit(None, Some(1)).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_eq!(json[0]["timestamp"], 1);

    output::set_json_options_with_limit(None, false, None);
}

#[test]
fn spread_and_funding_outputs_render_through_context_projection() {
    output::set_json_options_with_limit(Some("spread"), false, None);

    let spread = SpreadRow {
        coin: "BTC".to_string(),
        bid: dec("100"),
        ask: dec("101"),
        spread: dec("1"),
        spread_pct: dec("1"),
    };
    let spread_rendered = json_context(Some("coin,spread_pct")).render(&spread);
    let spread_json: serde_json::Value = serde_json::from_str(&spread_rendered).unwrap();
    assert_eq!(spread_json["coin"], "BTC");
    assert_eq!(spread_json["spread_pct"], "1");
    assert!(spread_json.get("spread").is_none());

    let funding = FundingRow {
        coin: "BTC".to_string(),
        current_funding_rate: Some(dec("0.0001")),
        predicted_funding_rate: dec("0.0002"),
        premium: dec("0.00005"),
        mark_price: Some(dec("100")),
        oracle_price: Some(dec("99")),
        last_funding_time: Some(123),
    };
    let funding_rendered = json_context(Some("coin,predicted_funding_rate")).render(&funding);
    let funding_json: serde_json::Value = serde_json::from_str(&funding_rendered).unwrap();
    assert_eq!(funding_json["coin"], "BTC");
    assert_eq!(funding_json["predicted_funding_rate"], "0.0002");
    assert!(funding_json.get("current_funding_rate").is_none());

    output::set_json_options_with_limit(None, false, None);
}
