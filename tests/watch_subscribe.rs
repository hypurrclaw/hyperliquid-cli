mod support;

use futures::stream;
use hyperliquid_cli::output;
use hyperliquid_cli::watch::{
    SubscribeEventKind, stream_subscription_events, subscription_event_matches,
};
use hypersdk::Address;
use hypersdk::hypercore::{
    BookLevel, Candle, Fill, Incoming, L2Book, NonUserCancel, OrderStatus, OrderUpdate, Side,
    Trade, UserEvent, UserFunding, UserLiquidation, WsBasicOrder, ws::Event as WsEvent,
};
use predicates::prelude::*;
use rust_decimal::Decimal;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use support::{API_OVERRIDE_ENV, FORMAT_ENV, IsolatedHome, mount_override_healthcheck};
use wiremock::MockServer;

const WATCH_MAX_TICKS_ENV: &str = "HYPERLIQUID_WATCH_MAX_TICKS";
const SUBSCRIBE_MAX_EVENTS_ENV: &str = "HYPERLIQUID_SUBSCRIBE_MAX_EVENTS";
static JSON_OPTIONS_TEST_LOCK: Mutex<()> = Mutex::new(());

struct JsonOptionsTestGuard {
    _guard: MutexGuard<'static, ()>,
}

impl Drop for JsonOptionsTestGuard {
    fn drop(&mut self) {
        output::set_json_options_with_limit(None, false, None);
    }
}

fn json_options_test_guard() -> JsonOptionsTestGuard {
    let guard = JSON_OPTIONS_TEST_LOCK.lock().unwrap();
    output::set_json_options_with_limit(None, false, None);
    JsonOptionsTestGuard { _guard: guard }
}

async fn mock_all_mids_server() -> MockServer {
    let server = MockServer::start().await;
    mount_override_healthcheck(&server).await;
    server
}

#[test]
fn watch_flags_are_exposed_on_required_commands() {
    let home = IsolatedHome::new();

    for args in [
        vec!["positions", "list", "--help"],
        vec!["orders", "open", "--help"],
        vec!["book", "--help"],
        vec!["mids", "--help"],
        vec!["candles", "--help"],
    ] {
        home.command()
            .args(args)
            .assert()
            .success()
            .stdout(predicate::str::contains("--watch"))
            .stdout(predicate::str::contains("--max-ticks"));
    }
}

#[tokio::test]
async fn watch_mids_json_max_ticks_cli_bounds_output_deterministically() {
    let home = IsolatedHome::new();
    let server = mock_all_mids_server().await;

    let output = home
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(WATCH_MAX_TICKS_ENV, "5")
        .args([
            "--select",
            "coin,price",
            "mids",
            "--watch",
            "--format",
            "json",
            "--max-ticks",
            "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1);

    let value: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(value[0]["coin"], "BTC");
    assert_eq!(value[0]["price"], "50000");
    assert_eq!(value[0].as_object().unwrap().len(), 2);
}

#[tokio::test]
async fn watch_mids_pretty_non_tty_with_bound_defaults_to_ndjson() {
    let home = IsolatedHome::new();
    let server = mock_all_mids_server().await;

    let output = home
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["mids", "--watch", "--max-ticks", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("EnterAlternateScreen").not())
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1);

    let value: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(value["BTC"], "50000");
    assert_eq!(value["ETH"], "3000");
}

#[tokio::test]
async fn watch_mids_table_non_tty_with_bound_defaults_to_ndjson() {
    let home = IsolatedHome::new();
    let server = mock_all_mids_server().await;

    let output = home
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["mids", "--watch", "--format", "table", "--max-ticks", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1);

    let value: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(value["BTC"], "50000");
    assert_eq!(value["ETH"], "3000");
}

#[tokio::test]
async fn watch_mids_pretty_non_tty_without_bound_errors_before_tui() {
    let home = IsolatedHome::new();
    let server = mock_all_mids_server().await;

    home.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "pretty", "mids", "--watch"])
        .assert()
        .failure()
        .code(13)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Non-TTY"))
        .stderr(predicate::str::contains("--format json"))
        .stderr(predicate::str::contains("--max-ticks"));
}

#[tokio::test]
async fn watch_mids_json_default_requires_bound_for_agents() {
    let home = IsolatedHome::new();
    let server = mock_all_mids_server().await;

    home.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env_remove(FORMAT_ENV)
        .args(["mids", "--watch"])
        .assert()
        .failure()
        .code(13)
        .stdout(predicate::str::contains(
            "JSON watch output must be bounded",
        ))
        .stdout(predicate::str::contains("--max-ticks"));
}

#[test]
fn watch_max_ticks_zero_is_rejected_by_cli_parser() {
    let home = IsolatedHome::new();

    home.command()
        .env_remove(FORMAT_ENV)
        .args(["mids", "--watch", "--max-ticks", "0"])
        .assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains("value must be at least 1"));
}

#[test]
fn subscribe_group_exposes_expected_subcommands_and_asset_flags() {
    let home = IsolatedHome::new();

    home.command()
        .args(["subscribe", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Automation callers"))
        .stdout(predicate::str::contains("--idle-timeout-ms"))
        .stdout(predicate::str::contains("trades"))
        .stdout(predicate::str::contains("orderbook"))
        .stdout(predicate::str::contains("candles"))
        .stdout(predicate::str::contains("all-mids"))
        .stdout(predicate::str::contains("order-updates"))
        .stdout(predicate::str::contains("fills"));

    for args in [
        ["subscribe", "trades", "--help"],
        ["subscribe", "orderbook", "--help"],
        ["subscribe", "candles", "--help"],
    ] {
        home.command()
            .args(args)
            .assert()
            .success()
            .stdout(predicate::str::contains("--asset"))
            .stdout(predicate::str::contains("--max-events"))
            .stdout(predicate::str::contains("--idle-timeout-ms"));
    }
}

#[test]
fn subscribe_all_mids_can_emit_subscribed_ndjson_and_exit_for_automation() {
    let home = IsolatedHome::new();

    let output = home
        .command()
        .args(["subscribe", "all-mids", "--max-events", "0"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    let first_line = stdout.lines().next().unwrap();
    let value: serde_json::Value = serde_json::from_str(first_line).unwrap();

    assert_eq!(value["type"], "subscribed");
    assert_eq!(value["subscription"], "allMids(None)");
}

#[test]
fn subscribe_json_default_requires_bound_for_agents() {
    let home = IsolatedHome::new();

    home.command()
        .env_remove(FORMAT_ENV)
        .args(["subscribe", "all-mids"])
        .assert()
        .failure()
        .code(13)
        .stdout(predicate::str::contains(
            "JSON subscribe output must be bounded",
        ))
        .stdout(predicate::str::contains("--max-events"));
}

#[test]
fn subscribe_agent_context_requires_bound_even_with_explicit_pretty_format() {
    let home = IsolatedHome::new();

    home.command()
        .env("HYPERLIQUID_AGENT", "1")
        .args(["--format", "pretty", "subscribe", "all-mids"])
        .assert()
        .failure()
        .code(13)
        .stderr(predicate::str::contains(
            "JSON subscribe output must be bounded",
        ));
}

#[test]
fn subscribe_json_accepts_env_bound_for_automation() {
    let home = IsolatedHome::new();

    let output = home
        .command()
        .env(SUBSCRIBE_MAX_EVENTS_ENV, "0")
        .args(["subscribe", "all-mids"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    let first_line = stdout.lines().next().unwrap();
    let value: serde_json::Value = serde_json::from_str(first_line).unwrap();

    assert_eq!(value["type"], "subscribed");
}

#[test]
fn subscribe_json_rejects_invalid_env_bound_for_agents() {
    let home = IsolatedHome::new();

    home.command()
        .env_remove(FORMAT_ENV)
        .env(SUBSCRIBE_MAX_EVENTS_ENV, "not-a-number")
        .args(["subscribe", "all-mids"])
        .assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains(
            "HYPERLIQUID_SUBSCRIBE_MAX_EVENTS must be a non-negative integer",
        ));
}

#[tokio::test]
async fn fake_subscribe_stream_applies_select_to_subscription_lines() {
    let _guard = json_options_test_guard();
    output::set_json_options_with_limit(Some("type,subscription"), false, None);
    let mut events = stream::iter([WsEvent::Message(all_mids_message())]);
    let mut output_bytes = Vec::new();

    stream_subscription_events(
        "allMids(None)",
        Some(0),
        None,
        |message| subscription_event_matches(SubscribeEventKind::AllMids, message),
        &mut events,
        &mut output_bytes,
    )
    .await
    .unwrap();

    let lines = ndjson_values(output_bytes);
    assert_eq!(
        lines[0],
        serde_json::json!({
            "type": "subscribed",
            "subscription": "allMids(None)"
        })
    );
    output::set_json_options_with_limit(None, false, None);
}

#[tokio::test]
async fn fake_subscribe_stream_limits_nested_all_mids_payload() {
    let _guard = json_options_test_guard();
    output::set_json_options_with_limit(None, false, Some(2));
    let mut events = stream::iter([WsEvent::Message(many_mids_message())]);
    let mut output_bytes = Vec::new();

    stream_subscription_events(
        "allMids(None)",
        Some(1),
        None,
        |message| subscription_event_matches(SubscribeEventKind::AllMids, message),
        &mut events,
        &mut output_bytes,
    )
    .await
    .unwrap();

    let lines = ndjson_values(output_bytes);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["type"], "event");
    assert_eq!(lines[1]["subscription"], "allMids(None)");
    let mids = lines[1]["data"]["data"]["mids"].as_object().unwrap();
    assert_eq!(mids.len(), 2);
    output::set_json_options_with_limit(None, false, None);
}

#[tokio::test]
async fn fake_subscribe_stream_emits_matching_event_envelopes() {
    let _guard = json_options_test_guard();
    let mut events = stream::iter([
        WsEvent::Connected,
        WsEvent::Message(all_mids_message()),
        WsEvent::Message(trades_message()),
    ]);
    let mut output = Vec::new();

    stream_subscription_events(
        "trades(BTC)",
        Some(1),
        None,
        |message| subscription_event_matches(SubscribeEventKind::Trades, message),
        &mut events,
        &mut output,
    )
    .await
    .unwrap();

    let lines = ndjson_values(output);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["type"], "subscribed");
    assert_eq!(lines[0]["subscription"], "trades(BTC)");
    assert_eq!(lines[0].as_object().unwrap().len(), 2);
    assert_eq!(lines[1]["type"], "event");
    assert_eq!(lines[1]["subscription"], "trades(BTC)");
    assert_eq!(lines[1]["data"]["channel"], "trades");
}

#[tokio::test]
async fn fake_subscribe_stream_ignores_non_matching_events_until_close() {
    let _guard = json_options_test_guard();
    let mut events = stream::iter([WsEvent::Message(all_mids_message())]);
    let mut output = Vec::new();

    let result = stream_subscription_events(
        "trades(BTC)",
        Some(1),
        None,
        |message| subscription_event_matches(SubscribeEventKind::Trades, message),
        &mut events,
        &mut output,
    )
    .await;

    let error = result.unwrap_err().to_string();
    assert!(error.contains("WebSocket closed before receiving subscription events"));

    let lines = ndjson_values(output);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["type"], "subscribed");
}

#[tokio::test]
async fn fake_subscribe_stream_max_events_counts_only_emitted_events() {
    let _guard = json_options_test_guard();
    let mut events = stream::iter([
        WsEvent::Message(trades_message()),
        WsEvent::Message(all_mids_message()),
        WsEvent::Disconnected,
        WsEvent::Message(trades_message()),
        WsEvent::Message(trades_message()),
    ]);
    let mut output = Vec::new();

    stream_subscription_events(
        "trades(BTC)",
        Some(2),
        None,
        |message| subscription_event_matches(SubscribeEventKind::Trades, message),
        &mut events,
        &mut output,
    )
    .await
    .unwrap();

    let lines = ndjson_values(output);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["type"], "subscribed");
    assert!(lines[1..].iter().all(|line| line["type"] == "event"));
    assert!(
        lines[1..]
            .iter()
            .all(|line| line["data"]["channel"] == "trades")
    );
}

#[tokio::test]
async fn fake_subscribe_stream_closed_stream_behavior_is_deterministic() {
    let _guard = json_options_test_guard();
    let mut events = stream::iter([]);
    let mut output = Vec::new();

    let result = stream_subscription_events(
        "allMids(None)",
        Some(1),
        None,
        |message| subscription_event_matches(SubscribeEventKind::AllMids, message),
        &mut events,
        &mut output,
    )
    .await;

    let error = result.unwrap_err().to_string();
    assert!(error.contains("WebSocket closed before receiving subscription events"));

    let lines = ndjson_values(output);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["type"], "subscribed");
    assert_eq!(lines[0]["subscription"], "allMids(None)");
}

#[tokio::test]
async fn fake_subscribe_stream_max_zero_emits_only_subscribed_envelope() {
    let _guard = json_options_test_guard();
    let mut events = stream::iter([WsEvent::Message(all_mids_message())]);
    let mut output = Vec::new();

    stream_subscription_events(
        "allMids(None)",
        Some(0),
        None,
        |message| subscription_event_matches(SubscribeEventKind::AllMids, message),
        &mut events,
        &mut output,
    )
    .await
    .unwrap();

    let lines = ndjson_values(output);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["type"], "subscribed");
}

#[tokio::test]
async fn fake_subscribe_stream_idle_timeout_bounds_quiet_max_events() {
    let _guard = json_options_test_guard();
    let mut events = stream::pending();
    let mut output = Vec::new();
    let started = Instant::now();

    let result = stream_subscription_events(
        "trades(BTC)",
        Some(1),
        Some(Duration::from_millis(5)),
        |message| subscription_event_matches(SubscribeEventKind::Trades, message),
        &mut events,
        &mut output,
    )
    .await;

    let error = result.unwrap_err().to_string();
    assert!(error.contains("Timed out waiting for subscription events after 5ms"));
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "idle timeout should bound quiet streams promptly"
    );

    let lines = ndjson_values(output);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["type"], "subscribed");
}

#[tokio::test]
async fn fake_fills_subscribe_stream_ignores_non_fill_user_events_before_fill() {
    let _guard = json_options_test_guard();
    let mut events = stream::iter([
        WsEvent::Message(user_events_funding_message()),
        WsEvent::Message(user_events_liquidation_message()),
        WsEvent::Message(user_events_non_user_cancel_message()),
        WsEvent::Message(user_events_unknown_message()),
        WsEvent::Message(user_events_fills_message()),
    ]);
    let mut output = Vec::new();

    stream_subscription_events(
        "userFills(0x0202020202020202020202020202020202020202)",
        Some(1),
        None,
        |message| subscription_event_matches(SubscribeEventKind::Fills, message),
        &mut events,
        &mut output,
    )
    .await
    .unwrap();

    let lines = ndjson_values(output);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["type"], "subscribed");
    assert_eq!(lines[1]["type"], "event");
    assert_eq!(lines[1]["data"]["channel"], "userEvents");
    assert!(lines[1]["data"]["data"].get("fills").is_some());
}

#[test]
fn subscription_event_matchers_cover_all_subscribe_wiring() {
    for (kind, matching_message) in [
        (SubscribeEventKind::Trades, trades_message()),
        (SubscribeEventKind::Orderbook, orderbook_message()),
        (SubscribeEventKind::Candles, candle_message()),
        (SubscribeEventKind::AllMids, all_mids_message()),
        (SubscribeEventKind::OrderUpdates, order_updates_message()),
        (SubscribeEventKind::Fills, user_fills_message()),
    ] {
        assert!(
            subscription_event_matches(kind, &matching_message),
            "{kind:?} should match its corresponding WebSocket event"
        );
        assert!(
            !subscription_event_matches(kind, &Incoming::Ping),
            "{kind:?} should ignore unrelated WebSocket events"
        );
    }

    assert!(subscription_event_matches(
        SubscribeEventKind::Fills,
        &user_events_fills_message()
    ));
}

#[test]
fn fills_subscription_ignores_non_fill_user_event_variants() {
    for non_fill_message in [
        user_events_funding_message(),
        user_events_liquidation_message(),
        user_events_non_user_cancel_message(),
        user_events_unknown_message(),
    ] {
        assert!(
            !subscription_event_matches(SubscribeEventKind::Fills, &non_fill_message),
            "fills subscription should ignore non-fill user event: {non_fill_message:?}"
        );
    }
}

fn ndjson_values(output: Vec<u8>) -> Vec<serde_json::Value> {
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn trades_message() -> Incoming {
    Incoming::Trades(vec![Trade {
        coin: "BTC".to_string(),
        side: Side::Bid,
        px: Decimal::from(50_000),
        sz: Decimal::new(1, 1),
        time: 1,
        hash: "0xabc".to_string(),
        tid: 42,
        users: [Address::ZERO, Address::repeat_byte(1)],
        liquidation: None,
    }])
}

fn orderbook_message() -> Incoming {
    Incoming::L2Book(L2Book {
        coin: "BTC".to_string(),
        time: 1,
        snapshot: true,
        levels: [
            vec![BookLevel {
                px: Decimal::from(49_999),
                sz: Decimal::new(2, 1),
                n: 1,
            }],
            vec![BookLevel {
                px: Decimal::from(50_001),
                sz: Decimal::new(3, 1),
                n: 1,
            }],
        ],
    })
}

fn candle_message() -> Incoming {
    Incoming::Candle(Candle {
        open_time: 1,
        close_time: 60_000,
        coin: "BTC".to_string(),
        interval: "1m".to_string(),
        open: Decimal::from(50_000),
        high: Decimal::from(50_100),
        low: Decimal::from(49_900),
        close: Decimal::from(50_050),
        volume: Decimal::new(25, 1),
        num_trades: 3,
    })
}

fn all_mids_message() -> Incoming {
    Incoming::AllMids {
        dex: None,
        mids: [("BTC".to_string(), Decimal::from(50_000))]
            .into_iter()
            .collect(),
    }
}

fn many_mids_message() -> Incoming {
    Incoming::AllMids {
        dex: None,
        mids: [
            ("BTC".to_string(), Decimal::from(50_000)),
            ("ETH".to_string(), Decimal::from(3_000)),
            ("SOL".to_string(), Decimal::from(100)),
        ]
        .into_iter()
        .collect(),
    }
}

fn order_updates_message() -> Incoming {
    Incoming::OrderUpdates(vec![OrderUpdate {
        status: OrderStatus::Open,
        status_timestamp: 1,
        order: WsBasicOrder {
            timestamp: 1,
            coin: "BTC".to_string(),
            side: Side::Bid,
            limit_px: Decimal::from(50_000),
            sz: Decimal::new(1, 1),
            oid: 7,
            orig_sz: Decimal::new(1, 1),
            cloid: None,
        },
    }])
}

fn user_fills_message() -> Incoming {
    Incoming::UserFills {
        is_snapshot: false,
        user: Address::repeat_byte(2),
        fills: vec![fill()],
    }
}

fn user_events_fills_message() -> Incoming {
    Incoming::UserEvents(UserEvent::Fills {
        fills: vec![fill()],
    })
}

fn user_events_funding_message() -> Incoming {
    Incoming::UserEvents(UserEvent::Funding {
        funding: UserFunding {
            time: 1,
            coin: "BTC".to_string(),
            usdc: Decimal::new(25, 2),
            szi: Decimal::new(1, 1),
            funding_rate: Decimal::new(1, 6),
        },
    })
}

fn user_events_liquidation_message() -> Incoming {
    Incoming::UserEvents(UserEvent::Liquidation {
        liquidation: UserLiquidation {
            lid: 7,
            liquidator: Address::repeat_byte(3),
            liquidated_user: Address::repeat_byte(4),
            liquidated_ntl_pos: Decimal::from(1_000),
            liquidated_account_value: Decimal::from(500),
        },
    })
}

fn user_events_non_user_cancel_message() -> Incoming {
    Incoming::UserEvents(UserEvent::NonUserCancel {
        non_user_cancel: vec![NonUserCancel {
            coin: "BTC".to_string(),
            oid: 11,
        }],
    })
}

fn user_events_unknown_message() -> Incoming {
    Incoming::UserEvents(UserEvent::Unknown(serde_json::json!({
        "unexpectedFutureUserEvent": {
            "coin": "BTC"
        }
    })))
}

fn fill() -> Fill {
    Fill {
        coin: "BTC".to_string(),
        px: Decimal::from(50_000),
        sz: Decimal::new(1, 1),
        side: Side::Ask,
        time: 1,
        start_position: Decimal::ZERO,
        dir: "Close Long".to_string(),
        closed_pnl: Decimal::from(5),
        hash: "0xdef".to_string(),
        oid: 9,
        crossed: true,
        fee: Decimal::new(1, 3),
        tid: 43,
        cloid: None,
        fee_token: "USDC".to_string(),
        liquidation: None,
    }
}
