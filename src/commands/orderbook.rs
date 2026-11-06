//! Order book and market data commands.
//!
//! Commands:
//! - `hyperliquid book <COIN>` — show L2 bids and asks
//! - `hyperliquid candles <COIN>` — show OHLCV candle history
//! - `hyperliquid spread <COIN>` — show best bid, ask, and spread
//! - `hyperliquid funding <COIN>` — show current and predicted funding
//! - `hyperliquid mids` — show all mid prices

use std::time::Duration;

use chrono::Utc;
use hypersdk::Decimal;
use hypersdk::hypercore::{
    AssetContext, BookLevel, Candle, CandleInterval, Chain, FundingRate, HttpClient, Incoming,
    L2Book, Subscription,
};
use serde::Serialize;

use crate::assets::{AssetResolver, Market};
use crate::commands::outcomes;
use crate::errors::CliError;
use crate::errors::from_sdk;
use crate::invocation::Invocation;
use crate::output::TableData;
use crate::ws::{Awaited, first_matching_message};

const WEBSOCKET_TIMEOUT: Duration = Duration::from_secs(15);
const VALID_INTERVALS: &str = "1m, 3m, 5m, 15m, 30m, 1h, 2h, 4h, 8h, 12h, 1d, 3d, 1w, 1M";

/// Parse and validate a candle interval for clap.
pub fn parse_candle_interval(input: &str) -> Result<CandleInterval, String> {
    input
        .parse::<CandleInterval>()
        .map_err(|_| format!("invalid interval '{input}'. Valid intervals: {VALID_INTERVALS}"))
}

/// Parse and validate the candle limit for clap.
pub fn parse_candle_limit(input: &str) -> Result<usize, String> {
    let limit = input
        .parse::<usize>()
        .map_err(|_| format!("invalid limit '{input}'. Limit must be an integer from 1 to 5000"))?;

    if (1..=5000).contains(&limit) {
        Ok(limit)
    } else {
        Err(format!(
            "invalid limit '{input}'. Limit must be an integer from 1 to 5000"
        ))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BookLevelRow {
    #[serde(with = "rust_decimal::serde::str")]
    pub price: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub size: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub depth: Decimal,
    pub orders: usize,
}

impl BookLevelRow {
    fn from_levels(levels: &[BookLevel]) -> Vec<Self> {
        let mut depth = Decimal::ZERO;
        levels
            .iter()
            .map(|level| {
                depth += level.sz;
                Self {
                    price: level.px,
                    size: level.sz,
                    depth,
                    orders: level.n,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BookSnapshot {
    pub coin: String,
    pub time: u64,
    pub bids: Vec<BookLevelRow>,
    pub asks: Vec<BookLevelRow>,
}

impl From<L2Book> for BookSnapshot {
    fn from(book: L2Book) -> Self {
        Self {
            coin: book.coin.clone(),
            time: book.time,
            bids: BookLevelRow::from_levels(book.bids()),
            asks: BookLevelRow::from_levels(book.asks()),
        }
    }
}

impl TableData for BookSnapshot {
    fn headers(&self) -> Vec<&str> {
        vec!["Side", "Price", "Size", "Depth", "Orders"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.bids
            .iter()
            .map(|level| {
                vec![
                    "Bid".to_string(),
                    level.price.to_string(),
                    level.size.to_string(),
                    level.depth.to_string(),
                    level.orders.to_string(),
                ]
            })
            .chain(self.asks.iter().map(|level| {
                vec![
                    "Ask".to_string(),
                    level.price.to_string(),
                    level.size.to_string(),
                    level.depth.to_string(),
                    level.orders.to_string(),
                ]
            }))
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CandleRow {
    pub timestamp: u64,
    pub close_time: u64,
    pub coin: String,
    pub interval: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub open: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub high: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub low: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub close: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub volume: Decimal,
    pub num_trades: u64,
}

impl From<Candle> for CandleRow {
    fn from(candle: Candle) -> Self {
        Self {
            timestamp: candle.open_time,
            close_time: candle.close_time,
            coin: candle.coin,
            interval: candle.interval,
            open: candle.open,
            high: candle.high,
            low: candle.low,
            close: candle.close,
            volume: candle.volume,
            num_trades: candle.num_trades,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandlesOutput {
    candles: Vec<CandleRow>,
}

impl CandlesOutput {
    #[must_use]
    pub fn new(candles: Vec<CandleRow>) -> Self {
        Self { candles }
    }
}

impl TableData for CandlesOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Timestamp",
            "Coin",
            "Interval",
            "Open",
            "High",
            "Low",
            "Close",
            "Volume",
            "Trades",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.candles
            .iter()
            .map(|candle| {
                vec![
                    crate::output::format::epoch_ms_to_iso(candle.timestamp),
                    candle.coin.clone(),
                    candle.interval.clone(),
                    candle.open.to_string(),
                    candle.high.to_string(),
                    candle.low.to_string(),
                    candle.close.to_string(),
                    candle.volume.to_string(),
                    candle.num_trades.to_string(),
                ]
            })
            .collect()
    }

    fn empty_message(&self) -> Option<&str> {
        Some("No candles.")
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.candles).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SpreadRow {
    pub coin: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub bid: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub ask: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub spread: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub spread_pct: Decimal,
}

impl TableData for SpreadRow {
    fn headers(&self) -> Vec<&str> {
        vec!["Coin", "Bid", "Ask", "Spread", "Spread %"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.coin.clone(),
            self.bid.to_string(),
            self.ask.to_string(),
            self.spread.to_string(),
            self.spread_pct.to_string(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FundingRow {
    pub coin: String,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub current_funding_rate: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str")]
    pub predicted_funding_rate: Decimal,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub premium: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub mark_price: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub oracle_price: Option<Decimal>,
    pub last_funding_time: Option<u64>,
}

impl TableData for FundingRow {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Coin",
            "Current Funding",
            "Predicted Funding",
            "Premium",
            "Mark",
            "Oracle",
            "Last Funding Time",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.coin.clone(),
            format_optional_decimal(self.current_funding_rate),
            self.predicted_funding_rate.to_string(),
            format_optional_decimal(self.premium),
            format_optional_decimal(self.mark_price),
            format_optional_decimal(self.oracle_price),
            self.last_funding_time
                .map(crate::output::format::epoch_ms_to_iso)
                .unwrap_or_else(|| "n/a".to_string()),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MidRow {
    pub name: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub mid: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidsOutput {
    mids: Vec<MidRow>,
    selected_fields: Option<Vec<String>>,
}

impl MidsOutput {
    #[must_use]
    pub fn new(mids: Vec<MidRow>, selected_fields: Option<&[String]>) -> Self {
        Self {
            mids,
            selected_fields: selected_fields.map(<[String]>::to_vec),
        }
    }
}

impl TableData for MidsOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Name", "Mid"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.mids
            .iter()
            .map(|mid| vec![mid.name.clone(), mid.mid.to_string()])
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        if let Some(fields) = &self.selected_fields {
            let rows = self
                .mids
                .iter()
                .map(|mid| {
                    let mut object = serde_json::Map::new();
                    for field in fields {
                        match field.as_str() {
                            "name" | "coin" => {
                                object.insert(field.clone(), serde_json::json!(mid.name));
                            }
                            "mid" | "price" => {
                                object.insert(field.clone(), serde_json::json!(mid.mid));
                            }
                            _ => {}
                        }
                    }
                    serde_json::Value::Object(object)
                })
                .collect::<Vec<_>>();

            serde_json::Value::Array(rows)
        } else {
            let mut object = serde_json::Map::new();
            for mid in &self.mids {
                object.insert(mid.name.clone(), serde_json::json!(mid.mid));
            }
            serde_json::Value::Object(object)
        }
    }
}

/// Resolve and render the L2 order book for a perpetual or spot market.
pub async fn book(inv: &Invocation, coin: &str) -> Result<(), anyhow::Error> {
    let resolver = inv.asset_resolver().await?;
    let snapshot = book_snapshot(inv.remote()?.chain(), &resolver, coin).await?;
    inv.print(&snapshot);
    Ok(())
}

/// Fetch one L2 book snapshot without printing it.
pub async fn book_snapshot(
    chain: Chain,
    resolver: &AssetResolver,
    coin: &str,
) -> Result<BookSnapshot, anyhow::Error> {
    let coin = resolve_book_coin(resolver, coin)?;
    let book = fetch_l2_book(chain, &coin).await?;
    Ok(BookSnapshot::from(book))
}

/// Resolve and render candle history for a perpetual market.
pub async fn candles(
    inv: &Invocation,
    coin: &str,
    interval: CandleInterval,
    limit: usize,
) -> Result<(), anyhow::Error> {
    let client = inv.remote()?.api().sdk();
    let resolver = inv.asset_resolver().await?;
    let snapshot = candles_snapshot(client, &resolver, coin, interval, limit).await?;
    inv.print(&snapshot);
    Ok(())
}

/// Fetch candle history without printing it.
pub async fn candles_snapshot(
    client: &HttpClient,
    resolver: &AssetResolver,
    coin: &str,
    interval: CandleInterval,
    limit: usize,
) -> Result<CandlesOutput, anyhow::Error> {
    let coin = resolve_perp_coin(resolver, coin)?;
    let end_time = Utc::now().timestamp_millis() as u64;
    let interval_ms = interval.to_duration().as_millis() as u64;
    let lookback = interval_ms.saturating_mul(limit.saturating_add(2) as u64);
    let start_time = end_time.saturating_sub(lookback);

    let mut candles = client
        .candle_snapshot(coin, interval, start_time, end_time)
        .await
        .map_err(from_sdk)?;

    candles.sort_by_key(|candle| candle.open_time);
    let skip = candles.len().saturating_sub(limit);
    let rows = candles
        .into_iter()
        .skip(skip)
        .map(CandleRow::from)
        .collect::<Vec<_>>();

    Ok(CandlesOutput::new(rows))
}

/// Resolve and render the best bid/ask spread for a perpetual market.
pub async fn spread(inv: &Invocation, coin: &str) -> Result<(), anyhow::Error> {
    let resolver = inv.asset_resolver().await?;
    let coin = resolve_perp_coin(&resolver, coin)?;
    let book = fetch_l2_book(inv.remote()?.chain(), &coin).await?;
    inv.print(&spread_row_from_book(&coin, &book)?);
    Ok(())
}

/// Resolve and render current plus predicted funding for a perpetual market.
pub async fn funding(inv: &Invocation, coin: &str) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let resolver = inv.asset_resolver().await?;
    let coin = resolve_perp_coin(&resolver, coin)?;
    let end_time = Utc::now().timestamp_millis() as u64;
    let start_time = end_time.saturating_sub(24 * 60 * 60 * 1000);

    let latest_history = remote
        .api()
        .sdk()
        .funding_history(coin.clone(), start_time, Some(end_time))
        .await
        .map_err(from_sdk)?
        .into_iter()
        .max_by_key(|rate| rate.time);

    let active_ctx = fetch_active_asset_ctx(remote.chain(), &coin).await?;
    inv.print(&FundingRow::from_history_and_context(
        &coin,
        latest_history,
        active_ctx,
    ));
    Ok(())
}

/// Fetch and render all mid prices.
pub async fn mids(inv: &Invocation) -> Result<(), anyhow::Error> {
    let selected = inv.session().json_options().selected_fields();
    inv.print(&mids_snapshot(inv.remote()?.api().sdk(), selected).await?);
    Ok(())
}

/// Fetch all mid prices without printing them. `selected` is `--select`: it
/// switches the JSON from a name-to-mid map to rows of the named fields.
pub async fn mids_snapshot(
    client: &HttpClient,
    selected: Option<&[String]>,
) -> Result<MidsOutput, anyhow::Error> {
    let mut mids = client
        .all_mids(None)
        .await
        .map_err(from_sdk)?
        .into_iter()
        .map(|(name, mid)| MidRow { name, mid })
        .collect::<Vec<_>>();

    mids.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(MidsOutput::new(mids, selected))
}

impl FundingRow {
    fn from_history_and_context(
        coin: &str,
        latest_history: Option<FundingRate>,
        ctx: AssetContext,
    ) -> Self {
        Self {
            coin: coin.to_string(),
            current_funding_rate: latest_history.as_ref().map(|rate| rate.funding_rate),
            predicted_funding_rate: ctx.funding,
            premium: ctx.premium,
            mark_price: ctx.mark_px,
            oracle_price: ctx.oracle_px,
            last_funding_time: latest_history.map(|rate| rate.time),
        }
    }
}

pub(crate) fn spread_row_from_book(coin: &str, book: &L2Book) -> Result<SpreadRow, CliError> {
    let best_bid = book.best_bid().ok_or_else(|| {
        CliError::Unavailable("Order book did not contain any bid levels.".to_string())
    })?;
    let best_ask = book.best_ask().ok_or_else(|| {
        CliError::Unavailable("Order book did not contain any ask levels.".to_string())
    })?;
    let spread = best_ask.px - best_bid.px;
    let spread_pct = if best_bid.px.is_zero() {
        Decimal::ZERO
    } else {
        (spread / best_bid.px * Decimal::from(100)).round_dp(6)
    };
    Ok(SpreadRow {
        coin: coin.to_string(),
        bid: best_bid.px,
        ask: best_ask.px,
        spread,
        spread_pct,
    })
}

fn resolve_book_coin(resolver: &AssetResolver, coin: &str) -> Result<String, CliError> {
    match resolver.resolve(coin)? {
        Market::Perp(perp) => Ok(perp.display_name()),
        Market::Spot(spot) => Ok(spot.info_coin()),
        Market::Outcome(_) => Err(outcomes::unsupported_outcome_trading_error(coin)),
    }
}

pub fn resolve_subscription_info_coin(
    resolver: &AssetResolver,
    coin: &str,
) -> Result<String, CliError> {
    resolve_book_coin(resolver, coin)
}

fn resolve_perp_coin(resolver: &AssetResolver, coin: &str) -> Result<String, CliError> {
    Ok(resolver.resolve_perp(coin)?.display_name())
}

async fn fetch_l2_book(chain: Chain, coin: &str) -> Result<L2Book, CliError> {
    first_matching_message(
        chain,
        Subscription::L2Book {
            coin: coin.to_string(),
            n_sig_figs: None,
            mantissa: None,
            fast: false,
        },
        WEBSOCKET_TIMEOUT,
        &Awaited {
            closed: "an order book snapshot",
            timed_out: "order book snapshot",
        },
        |message| match message {
            Incoming::L2Book(book) if book.coin.eq_ignore_ascii_case(coin) => Some(book),
            _ => None,
        },
    )
    .await
}

async fn fetch_active_asset_ctx(chain: Chain, coin: &str) -> Result<AssetContext, CliError> {
    first_matching_message(
        chain,
        Subscription::ActiveAssetCtx {
            coin: coin.to_string(),
        },
        WEBSOCKET_TIMEOUT,
        &Awaited {
            closed: "funding context",
            timed_out: "funding context",
        },
        |message| match message {
            Incoming::ActiveAssetCtx {
                coin: event_coin,
                ctx,
            } if event_coin.eq_ignore_ascii_case(coin) => Some(ctx),
            _ => None,
        },
    )
    .await
}

fn format_optional_decimal(value: Option<Decimal>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetMetadata;
    use crate::assets::test_support::{perp, spot};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(input: &str) -> Decimal {
        Decimal::from_str(input).unwrap()
    }

    fn fixture_resolver() -> AssetResolver {
        AssetResolver::new(AssetMetadata::from_assets(
            vec![perp("BTC", 0), perp("ETH", 1)],
            vec![
                spot("PURR/USDC", 10_000, "PURR", "USDC"),
                spot("HYPE/USDC", 11_035, "HYPE", "USDC"),
            ],
        ))
    }

    #[test]
    fn candle_interval_parser_accepts_supported_intervals() {
        assert_eq!(
            parse_candle_interval("15m").unwrap(),
            CandleInterval::FifteenMinutes
        );
        assert_eq!(
            parse_candle_interval("1h").unwrap(),
            CandleInterval::OneHour
        );
    }

    #[test]
    fn candle_interval_parser_rejects_invalid_intervals_with_valid_list() {
        let message = parse_candle_interval("999x").unwrap_err();

        assert!(message.contains("invalid interval '999x'"));
        assert!(message.contains("Valid intervals"));
        assert!(message.contains("15m"));
    }

    #[test]
    fn candle_limit_parser_rejects_zero_and_too_large_values() {
        assert!(parse_candle_limit("0").is_err());
        assert!(parse_candle_limit("5001").is_err());
        assert_eq!(parse_candle_limit("10").unwrap(), 10);
    }

    #[test]
    fn book_snapshot_json_has_bids_and_asks() {
        let output = BookSnapshot {
            coin: "BTC".to_string(),
            time: 123,
            bids: vec![BookLevelRow {
                price: dec("100"),
                size: dec("1.5"),
                depth: dec("1.5"),
                orders: 2,
            }],
            asks: vec![BookLevelRow {
                price: dec("101"),
                size: dec("2.0"),
                depth: dec("2.0"),
                orders: 3,
            }],
        };
        let json = output.to_json_value();

        assert!(json["bids"].is_array());
        assert!(json["asks"].is_array());
        assert_eq!(json["bids"][0]["price"], "100");
        assert_eq!(json["asks"][0]["size"], "2.0");
    }

    #[test]
    fn book_level_rows_include_cumulative_depth() {
        let levels = vec![
            BookLevel {
                px: dec("100"),
                sz: dec("1"),
                n: 1,
            },
            BookLevel {
                px: dec("99"),
                sz: dec("2.5"),
                n: 4,
            },
        ];

        let rows = BookLevelRow::from_levels(&levels);

        assert_eq!(rows[0].depth, dec("1"));
        assert_eq!(rows[1].depth, dec("3.5"));
    }

    #[test]
    fn book_resolution_accepts_perp_symbols() {
        let resolver = fixture_resolver();

        assert_eq!(resolve_book_coin(&resolver, "btc").unwrap(), "BTC");
    }

    #[test]
    fn book_resolution_accepts_spot_pairs() {
        let resolver = fixture_resolver();

        assert_eq!(
            resolve_book_coin(&resolver, "purr/usdc").unwrap(),
            "PURR/USDC"
        );
        assert_eq!(resolve_book_coin(&resolver, "hype/usdc").unwrap(), "@1035");
    }

    #[test]
    fn book_resolution_keeps_spot_pair_not_found_errors() {
        let resolver = fixture_resolver();
        let err = resolve_book_coin(&resolver, "PUR/USDC").unwrap_err();

        assert_eq!(err.exit_code(), 13);
        match err {
            CliError::AssetNotFound { suggestions, .. } => {
                assert!(suggestions.contains("PURR/USDC"))
            }
            other => panic!("expected spot pair suggestion, got {other:?}"),
        }
    }

    #[test]
    fn book_resolution_rejects_outcome_notation_with_outcome_guidance() {
        let resolver = fixture_resolver();
        let err = resolve_book_coin(&resolver, "#10").unwrap_err();

        assert_eq!(err.exit_code(), 13);
        assert!(err.to_string().contains("outcome-aware command path"));
        assert!(err.to_string().contains("outcomes get #10"));
    }

    #[test]
    fn candle_output_has_ohlcv_fields() {
        let output = CandlesOutput::new(vec![CandleRow {
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
        }]);
        let json = output.to_json_value();

        assert_eq!(json[0]["open"], "100");
        assert_eq!(json[0]["high"], "110");
        assert_eq!(json[0]["low"], "90");
        assert_eq!(json[0]["close"], "105");
        assert_eq!(json[0]["volume"], "42");
    }

    #[test]
    fn spread_output_includes_bid_ask_spread_and_percentage() {
        let output = SpreadRow {
            coin: "BTC".to_string(),
            bid: dec("100"),
            ask: dec("101"),
            spread: dec("1"),
            spread_pct: dec("1"),
        };
        let row = output.rows().remove(0);

        assert_eq!(row[1], "100");
        assert_eq!(row[2], "101");
        assert_eq!(row[3], "1");
        assert_eq!(row[4], "1");
    }

    #[test]
    fn funding_output_includes_current_and_predicted_rates() {
        let output = FundingRow {
            coin: "BTC".to_string(),
            current_funding_rate: Some(dec("0.0001")),
            predicted_funding_rate: dec("0.0002"),
            premium: Some(dec("0.00005")),
            mark_price: Some(dec("100")),
            oracle_price: Some(dec("99")),
            last_funding_time: Some(123),
        };
        let json = output.to_json_value();

        assert_eq!(json["current_funding_rate"], "0.0001");
        assert_eq!(json["predicted_funding_rate"], "0.0002");
        assert_eq!(json["premium"], "0.00005");
        assert_eq!(json["last_funding_time"], 123);
        assert_eq!(output.rows()[0][6], "1970-01-01T00:00:00.123Z");
    }

    #[test]
    fn mids_json_defaults_to_object_mapping_names_to_prices() {
        let output = MidsOutput::new(
            vec![
                MidRow {
                    name: "BTC".to_string(),
                    mid: dec("100"),
                },
                MidRow {
                    name: "ETH".to_string(),
                    mid: dec("50"),
                },
            ],
            None,
        );
        let json = output.to_json_value();

        assert!(json.is_object());
        assert_eq!(json["BTC"], "100");
        assert_eq!(json["ETH"], "50");
    }

    #[test]
    fn mids_select_filters_json_to_requested_fields() {
        let output = MidsOutput::new(
            vec![MidRow {
                name: "BTC".to_string(),
                mid: dec("100"),
            }],
            Some(&["name".to_string(), "mid".to_string()]),
        );
        let json = output.to_json_value();
        let object = json[0].as_object().unwrap();

        assert_eq!(object.len(), 2);
        assert_eq!(object["name"], "BTC");
        assert_eq!(object["mid"], "100");
    }
}
