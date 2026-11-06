//! `subscribe ...`: real-time streams.

use clap::Args;

use crate::assets::AssetResolver;
use crate::command_spec::{
    CliCommand, CommandSpec, GroupSpec, Lifecycle, OwsSupport, Risk, StreamBound, arg,
};
use crate::commands::orderbook::parse_candle_interval;
use crate::errors::CliError;
use crate::invocation::Invocation;
use crate::stream::{StreamBoundArgs, StreamBounds, SubscribeTarget};

pub const GROUP: GroupSpec = GroupSpec::new(&["subscribe"], "Stream real-time market and account events")
    .long_about("Stream real-time market and account events over WebSocket.\n\
\n\
Automation callers should bound streams with --max-events and/or --idle-timeout-ms so commands return. Global JSON shaping flags such as --select and --max-results apply to JSONL stream lines.");

#[derive(Args, Debug, Clone)]
pub struct SubscribeAssetArgs {
    /// Asset name (e.g., BTC)
    #[arg(long)]
    pub asset: String,
    #[command(flatten)]
    pub stream: StreamBoundArgs,
}

#[derive(Args, Debug, Clone)]
pub struct SubscribeCandlesArgs {
    /// Asset name (e.g., BTC)
    #[arg(long)]
    pub asset: String,
    /// Candle interval
    #[arg(long, default_value = "1m")]
    pub interval: String,
    #[command(flatten)]
    pub stream: StreamBoundArgs,
}

#[derive(Args, Debug, Clone)]
pub struct SubscribeStreamArgs {
    #[command(flatten)]
    pub stream: StreamBoundArgs,
}

pub struct SubscribeTrades;

impl CliCommand for SubscribeTrades {
    type Args = SubscribeAssetArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["subscribe", "trades"],
        "subscribe",
        "Stream real-time trades",
        "Stream real-time trades. automation callers should set max_events or idle_timeout_ms so the call returns.",
    )
    .lifecycle(Lifecycle::Streaming)
    .stream(StreamBound::Subscribe)
    .args(&[
        arg("asset").describe("Asset name, for example BTC.").resource_id("asset"),
        arg("max_events").describe("Stop after this many matching events."),
        arg("idle_timeout_ms").describe("Stop if no matching events arrive within this many milliseconds."),
    ]);

    async fn run(inv: &Invocation, args: &SubscribeAssetArgs) -> Result<(), anyhow::Error> {
        let resolver = inv.asset_resolver().await?;
        let coin = resolve_subscription_coin(&resolver, &args.asset)?;
        stream(inv, SubscribeTarget::trades(coin), &args.stream).await
    }
}

pub struct SubscribeOrderbook;

impl CliCommand for SubscribeOrderbook {
    type Args = SubscribeAssetArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["subscribe", "orderbook"],
        "subscribe",
        "Stream L2 order book updates",
        "Stream L2 order book updates. automation callers should set max_events or idle_timeout_ms so the call returns.",
    )
    .lifecycle(Lifecycle::Streaming)
    .stream(StreamBound::Subscribe)
    .args(&[
        arg("asset").describe("Asset name, for example BTC.").resource_id("asset"),
        arg("max_events").describe("Stop after this many matching events."),
        arg("idle_timeout_ms").describe("Stop if no matching events arrive within this many milliseconds."),
    ]);

    async fn run(inv: &Invocation, args: &SubscribeAssetArgs) -> Result<(), anyhow::Error> {
        let resolver = inv.asset_resolver().await?;
        let coin = resolve_subscription_coin(&resolver, &args.asset)?;
        stream(inv, SubscribeTarget::orderbook(coin), &args.stream).await
    }
}

pub struct SubscribeCandles;

impl CliCommand for SubscribeCandles {
    type Args = SubscribeCandlesArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["subscribe", "candles"],
        "subscribe",
        "Stream candle updates",
        "Stream candle updates. automation callers should set max_events or idle_timeout_ms so the call returns.",
    )
    .lifecycle(Lifecycle::Streaming)
    .stream(StreamBound::Subscribe)
    .args(&[
        arg("asset").describe("Asset name, for example BTC.").resource_id("asset"),
        arg("interval").describe("Candle interval such as 1m, 5m, 1h, or 1d."),
        arg("max_events").describe("Stop after this many matching events."),
        arg("idle_timeout_ms").describe("Stop if no matching events arrive within this many milliseconds."),
    ]);

    async fn run(inv: &Invocation, args: &SubscribeCandlesArgs) -> Result<(), anyhow::Error> {
        parse_candle_interval(&args.interval).map_err(CliError::Configuration)?;
        let resolver = inv.asset_resolver().await?;
        let coin = resolve_subscription_coin(&resolver, &args.asset)?;
        stream(
            inv,
            SubscribeTarget::candles(coin, args.interval.clone()),
            &args.stream,
        )
        .await
    }
}

pub struct SubscribeAllMids;

impl CliCommand for SubscribeAllMids {
    type Args = SubscribeStreamArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["subscribe", "all-mids"],
        "subscribe",
        "Stream all mid price updates",
        "Stream all mid-price updates. automation callers should set max_events or idle_timeout_ms so the call returns.",
    )
    .lifecycle(Lifecycle::Streaming)
    .stream(StreamBound::Subscribe)
    .args(&[
        arg("max_events").describe("Stop after this many matching events."),
        arg("idle_timeout_ms").describe("Stop if no matching events arrive within this many milliseconds."),
    ]);

    async fn run(inv: &Invocation, args: &SubscribeStreamArgs) -> Result<(), anyhow::Error> {
        stream(inv, SubscribeTarget::all_mids(), &args.stream).await
    }
}

pub struct SubscribeOrderUpdates;

impl CliCommand for SubscribeOrderUpdates {
    type Args = SubscribeStreamArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["subscribe", "order-updates"],
        "subscribe",
        "Stream order status updates (authenticated)",
        "Stream authenticated order status updates. automation callers should set max_events or idle_timeout_ms so the call returns.",
    )
    .lifecycle(Lifecycle::Streaming)
    .risk(Risk::AccountState)
    .ows(OwsSupport::ExperimentalFeatureGated)
    .auth(true)
    .stream(StreamBound::Subscribe)
    .args(&[
        arg("max_events").describe("Stop after this many matching events."),
        arg("idle_timeout_ms").describe("Stop if no matching events arrive within this many milliseconds."),
    ]);

    async fn run(inv: &Invocation, args: &SubscribeStreamArgs) -> Result<(), anyhow::Error> {
        let user = inv.resolve_signer()?.query_address();
        stream(inv, SubscribeTarget::order_updates(user), &args.stream).await
    }
}

pub struct SubscribeFills;

impl CliCommand for SubscribeFills {
    type Args = SubscribeStreamArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["subscribe", "fills"],
        "subscribe",
        "Stream fill events (authenticated)",
        "Stream authenticated fill events. automation callers should set max_events or idle_timeout_ms so the call returns.",
    )
    .lifecycle(Lifecycle::Streaming)
    .risk(Risk::AccountState)
    .ows(OwsSupport::ExperimentalFeatureGated)
    .auth(true)
    .stream(StreamBound::Subscribe)
    .args(&[
        arg("max_events").describe("Stop after this many matching events."),
        arg("idle_timeout_ms").describe("Stop if no matching events arrive within this many milliseconds."),
    ]);

    async fn run(inv: &Invocation, args: &SubscribeStreamArgs) -> Result<(), anyhow::Error> {
        let user = inv.resolve_signer()?.query_address();
        stream(inv, SubscribeTarget::fills(user), &args.stream).await
    }
}

fn resolve_subscription_coin(resolver: &AssetResolver, asset: &str) -> Result<String, CliError> {
    crate::commands::orderbook::resolve_subscription_info_coin(resolver, asset)
}

async fn stream(
    inv: &Invocation,
    target: SubscribeTarget,
    bounds: &StreamBoundArgs,
) -> Result<(), anyhow::Error> {
    crate::stream::stream_subscription(
        inv.session(),
        inv.remote()?.chain(),
        target,
        StreamBounds::subscribe(bounds),
    )
    .await
}
