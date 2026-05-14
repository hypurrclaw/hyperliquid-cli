//! Order book and market data commands.
//!
//! Commands:
//! - `hyperliquid book <COIN>` — show L2 bids and asks
//! - `hyperliquid candles <COIN>` — show OHLCV candle history
//! - `hyperliquid spread <COIN>` — show best bid, ask, and spread
//! - `hyperliquid funding <COIN>` — show current and predicted funding
//! - `hyperliquid mids` — show all mid prices

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use chrono::Utc;
use futures::StreamExt;
use hypersdk::Decimal;
use hypersdk::hypercore::{
    self, AssetContext, BookLevel, Candle, CandleInterval, Chain, FundingRate, HttpClient,
    Incoming, L2Book, Subscription, ws::Event,
};
use serde::Serialize;

use crate::command_context::CommandContext;
use crate::commands::{AssetResolver, ResolvedAsset, map_api_error};
use crate::errors::CliError;
use crate::output::{OutputFormat, TableData};

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
                    candle.timestamp.to_string(),
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
    #[serde(with = "rust_decimal::serde::str")]
    pub premium: Decimal,
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
            self.premium.to_string(),
            format_optional_decimal(self.mark_price),
            format_optional_decimal(self.oracle_price),
            self.last_funding_time
                .map(|time| time.to_string())
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
    pub fn new(mids: Vec<MidRow>, select: Option<&str>) -> Self {
        let selected_fields = select.map(parse_selected_fields);
        Self {
            mids,
            selected_fields,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookResult {
    pub output: BookSnapshot,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandlesResult {
    pub output: CandlesOutput,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpreadResult {
    pub output: SpreadRow,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingResult {
    pub output: FundingRow,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidsResult {
    pub output: MidsOutput,
    pub elapsed: Duration,
}

/// Resolve and render the L2 order book for a perpetual or spot market.
pub async fn book(
    chain: Chain,
    resolver: &AssetResolver,
    coin: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = book_query(chain, resolver, coin).await?;

    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Resolve and fetch the L2 order book for a perpetual or spot market.
pub async fn book_query(
    chain: Chain,
    resolver: &AssetResolver,
    coin: &str,
) -> Result<BookResult, anyhow::Error> {
    let start = Instant::now();
    let output = book_snapshot(chain, resolver, coin).await?;

    Ok(BookResult {
        output,
        elapsed: start.elapsed(),
    })
}

/// Resolve and render the L2 order book through a per-call output context.
pub async fn book_with_context(
    context: &CommandContext<'_>,
    chain: Chain,
    resolver: &AssetResolver,
    coin: &str,
) -> Result<(), anyhow::Error> {
    let result = book_query(chain, resolver, coin).await?;
    context.print(&result.output, result.elapsed);
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
    client: &HttpClient,
    resolver: &AssetResolver,
    coin: &str,
    interval: CandleInterval,
    limit: usize,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = candles_query(client, resolver, coin, interval, limit).await?;

    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Resolve and fetch candle history for a perpetual market.
pub async fn candles_query(
    client: &HttpClient,
    resolver: &AssetResolver,
    coin: &str,
    interval: CandleInterval,
    limit: usize,
) -> Result<CandlesResult, anyhow::Error> {
    let start = Instant::now();
    let output = candles_snapshot(client, resolver, coin, interval, limit).await?;

    Ok(CandlesResult {
        output,
        elapsed: start.elapsed(),
    })
}

/// Resolve and render candle history through a per-call output context.
pub async fn candles_with_context(
    context: &CommandContext<'_>,
    resolver: &AssetResolver,
    coin: &str,
    interval: CandleInterval,
    limit: usize,
) -> Result<(), anyhow::Error> {
    let client = context
        .hypercore_client()
        .ok_or_else(|| anyhow::anyhow!("candles command requires a Hyperliquid HTTP client"))?;
    let result = candles_query(client, resolver, coin, interval, limit).await?;
    context.print(&result.output, result.elapsed);
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
        .map_err(map_api_error)?;

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
pub async fn spread(
    chain: Chain,
    resolver: &AssetResolver,
    coin: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = spread_query(chain, resolver, coin).await?;

    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Resolve and fetch the best bid/ask spread for a perpetual market.
pub async fn spread_query(
    chain: Chain,
    resolver: &AssetResolver,
    coin: &str,
) -> Result<SpreadResult, anyhow::Error> {
    let start = Instant::now();
    let output = spread_snapshot(chain, resolver, coin).await?;

    Ok(SpreadResult {
        output,
        elapsed: start.elapsed(),
    })
}

/// Fetch the best bid/ask spread for a perpetual market without printing it.
pub async fn spread_snapshot(
    chain: Chain,
    resolver: &AssetResolver,
    coin: &str,
) -> Result<SpreadRow, anyhow::Error> {
    let coin = resolve_perp_coin(resolver, coin)?;
    let book = fetch_l2_book(chain, &coin).await?;
    spread_row_from_book(&coin, &book).map_err(Into::into)
}

/// Resolve and render the best bid/ask spread through a per-call output context.
pub async fn spread_with_context(
    context: &CommandContext<'_>,
    chain: Chain,
    resolver: &AssetResolver,
    coin: &str,
) -> Result<(), anyhow::Error> {
    let result = spread_query(chain, resolver, coin).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

/// Resolve and render current plus predicted funding for a perpetual market.
pub async fn funding(
    client: &HttpClient,
    chain: Chain,
    resolver: &AssetResolver,
    coin: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = funding_query(client, chain, resolver, coin).await?;

    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Resolve and fetch current plus predicted funding for a perpetual market.
pub async fn funding_query(
    client: &HttpClient,
    chain: Chain,
    resolver: &AssetResolver,
    coin: &str,
) -> Result<FundingResult, anyhow::Error> {
    let start = Instant::now();
    let output = funding_snapshot(client, chain, resolver, coin).await?;

    Ok(FundingResult {
        output,
        elapsed: start.elapsed(),
    })
}

/// Fetch current plus predicted funding for a perpetual market without printing it.
pub async fn funding_snapshot(
    client: &HttpClient,
    chain: Chain,
    resolver: &AssetResolver,
    coin: &str,
) -> Result<FundingRow, anyhow::Error> {
    let coin = resolve_perp_coin(resolver, coin)?;
    let end_time = Utc::now().timestamp_millis() as u64;
    let start_time = end_time.saturating_sub(24 * 60 * 60 * 1000);

    let latest_history = client
        .funding_history(coin.clone(), start_time, Some(end_time))
        .await
        .map_err(map_api_error)?
        .into_iter()
        .max_by_key(|rate| rate.time);

    let active_ctx = fetch_active_asset_ctx(chain, &coin).await?;
    Ok(funding_row_from_history_and_context(
        &coin,
        latest_history,
        active_ctx,
    ))
}

/// Resolve and render current plus predicted funding through a per-call output context.
pub async fn funding_with_context(
    context: &CommandContext<'_>,
    chain: Chain,
    resolver: &AssetResolver,
    coin: &str,
) -> Result<(), anyhow::Error> {
    let client = context
        .hypercore_client()
        .ok_or_else(|| anyhow::anyhow!("funding command requires a Hyperliquid HTTP client"))?;
    let result = funding_query(client, chain, resolver, coin).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

/// Fetch and render all mid prices.
pub async fn mids(
    client: &HttpClient,
    format: OutputFormat,
    select: Option<&str>,
) -> Result<(), anyhow::Error> {
    let result = mids_query(client, select).await?;

    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Fetch all mid prices.
pub async fn mids_query(
    client: &HttpClient,
    select: Option<&str>,
) -> Result<MidsResult, anyhow::Error> {
    let start = Instant::now();
    let output = mids_snapshot(client, select).await?;

    Ok(MidsResult {
        output,
        elapsed: start.elapsed(),
    })
}

/// Fetch and render all mid prices through a per-call output context.
pub async fn mids_with_context(
    context: &CommandContext<'_>,
    select: Option<&str>,
) -> Result<(), anyhow::Error> {
    let client = context
        .hypercore_client()
        .ok_or_else(|| anyhow::anyhow!("mids command requires a Hyperliquid HTTP client"))?;
    let result = mids_query(client, select).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

/// Fetch all mid prices without printing them.
pub async fn mids_snapshot(
    client: &HttpClient,
    select: Option<&str>,
) -> Result<MidsOutput, anyhow::Error> {
    let mut mids = client
        .all_mids(None)
        .await
        .map_err(map_api_error)?
        .into_iter()
        .map(|(name, mid)| MidRow { name, mid })
        .collect::<Vec<_>>();

    mids.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(MidsOutput::new(mids, select))
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

pub(crate) fn funding_row_from_history_and_context(
    coin: &str,
    latest_history: Option<FundingRate>,
    ctx: AssetContext,
) -> FundingRow {
    FundingRow::from_history_and_context(coin, latest_history, ctx)
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
        ResolvedAsset::Perp { name, dex, .. } => Ok(match dex {
            Some(dex) => format!("{dex}:{name}"),
            None => name,
        }),
        ResolvedAsset::Spot {
            symbol,
            index,
            base,
            quote,
            ..
        } => Ok(book_coin_for_spot(&symbol, index, &base, &quote)),
    }
}

pub fn resolve_subscription_info_coin(
    resolver: &AssetResolver,
    coin: &str,
) -> Result<String, CliError> {
    resolve_book_coin(resolver, coin)
}

fn book_coin_for_spot(symbol: &str, index: usize, base: &str, quote: &str) -> String {
    if base.eq_ignore_ascii_case("PURR") && quote.eq_ignore_ascii_case("USDC") && index == 10_000 {
        symbol.to_string()
    } else {
        format!("@{}", index.saturating_sub(10_000))
    }
}

fn resolve_perp_coin(resolver: &AssetResolver, coin: &str) -> Result<String, CliError> {
    match resolver.resolve_perp(coin)? {
        ResolvedAsset::Perp { name, dex, .. } => Ok(match dex {
            Some(dex) => format!("{dex}:{name}"),
            None => name,
        }),
        ResolvedAsset::Spot { .. } => Err(CliError::AssetNotFoundNoSuggestion {
            asset: coin.to_string(),
        }),
    }
}

async fn fetch_l2_book(chain: Chain, coin: &str) -> Result<L2Book, CliError> {
    let mut ws = websocket_for_chain(chain);
    ws.subscribe(Subscription::L2Book {
        coin: coin.to_string(),
        n_sig_figs: None,
        mantissa: None,
    });

    tokio::time::timeout(WEBSOCKET_TIMEOUT, async {
        while let Some(event) = ws.next().await {
            if let Event::Message(Incoming::L2Book(book)) = event
                && book.coin.eq_ignore_ascii_case(coin)
            {
                return Ok(book);
            }
        }

        Err(CliError::Unavailable(
            "WebSocket closed before receiving an order book snapshot.".to_string(),
        ))
    })
    .await
    .map_err(|_| {
        CliError::Unavailable(
            "Timed out waiting for Hyperliquid order book snapshot. Check your network connection."
                .to_string(),
        )
    })?
}

async fn fetch_active_asset_ctx(chain: Chain, coin: &str) -> Result<AssetContext, CliError> {
    let mut ws = websocket_for_chain(chain);
    ws.subscribe(Subscription::ActiveAssetCtx {
        coin: coin.to_string(),
    });

    tokio::time::timeout(WEBSOCKET_TIMEOUT, async {
        while let Some(event) = ws.next().await {
            if let Event::Message(Incoming::ActiveAssetCtx {
                coin: event_coin,
                ctx,
            }) = event
                && event_coin.eq_ignore_ascii_case(coin)
            {
                return Ok(ctx);
            }
        }

        Err(CliError::Unavailable(
            "WebSocket closed before receiving funding context.".to_string(),
        ))
    })
    .await
    .map_err(|_| {
        CliError::Unavailable(
            "Timed out waiting for Hyperliquid funding context. Check your network connection."
                .to_string(),
        )
    })?
}

fn websocket_for_chain(chain: Chain) -> hypercore::WebSocket {
    match chain {
        Chain::Mainnet => hypercore::mainnet_ws(),
        Chain::Testnet => hypercore::testnet_ws(),
    }
}

fn parse_selected_fields(select: &str) -> Vec<String> {
    select
        .split(',')
        .map(|field| field.trim().to_ascii_lowercase())
        .filter(|field| !field.is_empty())
        .collect()
}

fn format_optional_decimal(value: Option<Decimal>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

#[allow(dead_code)]
fn sorted_decimal_map(rows: &[MidRow]) -> BTreeMap<String, Decimal> {
    rows.iter()
        .map(|row| (row.name.clone(), row.mid))
        .collect::<BTreeMap<_, _>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{AssetMetadata, PerpAsset, SpotAsset};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(input: &str) -> Decimal {
        Decimal::from_str(input).unwrap()
    }

    fn fixture_resolver() -> AssetResolver {
        AssetResolver::new(AssetMetadata::from_assets(
            vec![
                PerpAsset::default_dex("BTC", 0),
                PerpAsset::default_dex("ETH", 1),
            ],
            vec![
                SpotAsset::new("PURR/USDC", 10_000, "PURR", "USDC"),
                SpotAsset::new("HYPE/USDC", 11_035, "HYPE", "USDC"),
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
            premium: dec("0.00005"),
            mark_price: Some(dec("100")),
            oracle_price: Some(dec("99")),
            last_funding_time: Some(123),
        };
        let json = output.to_json_value();

        assert_eq!(json["current_funding_rate"], "0.0001");
        assert_eq!(json["predicted_funding_rate"], "0.0002");
        assert_eq!(json["premium"], "0.00005");
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
            Some("name,mid"),
        );
        let json = output.to_json_value();
        let object = json[0].as_object().unwrap();

        assert_eq!(object.len(), 2);
        assert_eq!(object["name"], "BTC");
        assert_eq!(object["mid"], "100");
    }
}
