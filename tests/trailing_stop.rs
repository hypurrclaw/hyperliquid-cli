mod support;

use predicates::prelude::*;
use serde_json::{Value, json};
use support::{
    API_OVERRIDE_ENV, IsolatedHome, PRIVATE_KEY_ENV, VALID_PRIVATE_KEY,
    mount_common_public_endpoints,
};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OID: u64 = 77_738_308;

fn trailing_stop_ok(oid: u64) -> Value {
    json!({
        "status": "ok",
        "response": {"type": "trailingStop", "data": {"oid": oid}}
    })
}

async fn trading_server(exchange_response: Value) -> MockServer {
    let server = MockServer::start().await;
    mount_common_public_endpoints(&server).await;
    Mock::given(method("POST"))
        .and(path("/exchange"))
        .respond_with(ResponseTemplate::new(200).set_body_json(exchange_response))
        .mount(&server)
        .await;
    server
}

async fn exchange_bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == "/exchange")
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect()
}

/// Requests other than the `allMids` reachability probe every API override gets.
async fn lookup_and_exchange_requests(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
            format!(
                "{} {}",
                request.url.path(),
                body["type"].as_str().unwrap_or("")
            )
        })
        .filter(|summary| summary != "/info allMids")
        .collect()
}

/// A `0x` scalar of at most `max_digits` hex digits. The SDK writes `r` and `s`
/// as integers, so leading zeros are not padded.
fn is_hex_scalar(value: &Value, max_digits: usize) -> bool {
    value.as_str().is_some_and(|text| {
        text.strip_prefix("0x").is_some_and(|hex| {
            (1..=max_digits).contains(&hex.len()) && hex.chars().all(|ch| ch.is_ascii_hexdigit())
        })
    })
}

fn json_stdout(output: &assert_cmd::assert::Assert) -> Value {
    serde_json::from_slice(&output.get_output().stdout).unwrap()
}

#[tokio::test]
async fn testnet_submit_posts_one_trailing_stop_action_and_prints_the_oid() {
    let env = IsolatedHome::new();
    let server = trading_server(trailing_stop_ok(OID)).await;

    let assert = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "trailing-stop",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--size",
            "0.01",
            "--retracement-pct",
            "5",
            "--reduce-only",
            "--testnet",
        ])
        .assert()
        .success();

    let bodies = exchange_bodies(&server).await;
    assert_eq!(bodies.len(), 1, "exactly one /exchange request");
    let body = &bodies[0];
    assert_eq!(
        body["action"],
        json!({
            "type": "trailingStop",
            "asset": 0,
            "isBuy": false,
            "sz": "0.01",
            "reduceOnly": true,
            "retracement": {"pct": "5%"},
            "activationPx": null,
        })
    );
    assert!(body["nonce"].as_u64().is_some_and(|nonce| nonce > 0));
    assert!(is_hex_scalar(&body["signature"]["r"], 64), "{body}");
    assert!(is_hex_scalar(&body["signature"]["s"], 64), "{body}");
    assert!(matches!(body["signature"]["v"].as_u64(), Some(27 | 28)));
    assert!(body["vaultAddress"].is_null());

    let rows = json_stdout(&assert);
    let receipt = &rows[0];
    assert_eq!(receipt["oid"], OID);
    assert_eq!(receipt["coin"], "BTC");
    assert_eq!(receipt["side"], "sell");
    assert_eq!(receipt["size"], "0.01");
    assert_eq!(receipt["reduce_only"], true);
    assert_eq!(receipt["retracement"], json!({"pct": "5"}));
    assert!(receipt["activation_px"].is_null());
    assert_eq!(receipt["status"], "waiting_for_trigger");
}

#[tokio::test]
async fn price_distance_and_activation_price_use_their_wire_values() {
    let env = IsolatedHome::new();
    let server = trading_server(trailing_stop_ok(OID)).await;

    let assert = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "trailing-stop",
            "--coin",
            "ETH",
            "--side",
            "buy",
            "--size",
            "0.5",
            "--retracement-px",
            "50.50",
            "--activation-price",
            "2900",
            "--testnet",
        ])
        .assert()
        .success();

    let bodies = exchange_bodies(&server).await;
    assert_eq!(bodies.len(), 1);
    assert_eq!(
        bodies[0]["action"],
        json!({
            "type": "trailingStop",
            "asset": 1,
            "isBuy": true,
            "sz": "0.5",
            "reduceOnly": false,
            "retracement": {"px": "50.5"},
            "activationPx": "2900",
        })
    );
    let receipt = &json_stdout(&assert)[0];
    assert_eq!(receipt["retracement"], json!({"px": "50.5"}));
    assert_eq!(receipt["activation_px"], "2900");
}

#[tokio::test]
async fn on_behalf_of_sends_the_vault_address() {
    let env = IsolatedHome::new();
    let server = trading_server(trailing_stop_ok(OID)).await;
    let vault = "0x00000000000000000000000000000000000000aa";

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "trailing-stop",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--size",
            "0.01",
            "--retracement-pct",
            "2.5%",
            "--on-behalf-of",
            vault,
            "--testnet",
        ])
        .assert()
        .success();

    let bodies = exchange_bodies(&server).await;
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["vaultAddress"], vault);
    assert_eq!(bodies[0]["action"]["retracement"], json!({"pct": "2.5%"}));
}

#[tokio::test]
async fn pretty_output_lists_the_receipt_and_the_cancel_command() {
    let env = IsolatedHome::new();
    let server = trading_server(trailing_stop_ok(OID)).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "trailing-stop",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--size",
            "0.01",
            "--retracement-pct",
            "5",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("77738308"))
        .stdout(predicate::str::contains("waiting_for_trigger"))
        .stdout(predicate::str::contains("5%"))
        .stderr(predicate::str::contains("Cancel it:"))
        .stderr(predicate::str::contains(
            "hyperliquid orders cancel 77738308",
        ));
}

#[tokio::test]
async fn both_or_neither_retracement_option_is_a_usage_error_before_any_request() {
    let env = IsolatedHome::new();
    let server = trading_server(trailing_stop_ok(OID)).await;
    let base = [
        "orders",
        "trailing-stop",
        "--coin",
        "BTC",
        "--side",
        "sell",
        "--size",
        "0.01",
        "--testnet",
    ];

    for extra in [
        vec![],
        vec!["--retracement-pct", "5", "--retracement-px", "100"],
    ] {
        env.command()
            .env(API_OVERRIDE_ENV, server.uri())
            .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
            .args(base.iter().chain(extra.iter()))
            .assert()
            .code(2);
    }
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "usage errors must not reach the network"
    );
}

#[tokio::test]
async fn out_of_range_retracement_is_rejected_before_any_request() {
    let env = IsolatedHome::new();
    let server = trading_server(trailing_stop_ok(OID)).await;

    for extra in [
        ["--retracement-pct", "0"],
        ["--retracement-pct", "100"],
        ["--retracement-pct", "-5"],
        ["--retracement-px", "0"],
        ["--retracement-px", "-1"],
    ] {
        env.command()
            .env(API_OVERRIDE_ENV, server.uri())
            .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
            .args([
                "orders",
                "trailing-stop",
                "--coin",
                "BTC",
                "--side",
                "sell",
                "--size",
                "0.01",
                "--testnet",
            ])
            .args(extra)
            .assert()
            .code(2);
    }
    assert_eq!(
        lookup_and_exchange_requests(&server).await,
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn spot_and_outcome_markets_are_rejected_before_any_request() {
    let env = IsolatedHome::new();
    let server = trading_server(trailing_stop_ok(OID)).await;

    for coin in ["HYPE/USDC", "#10"] {
        env.command()
            .env(API_OVERRIDE_ENV, server.uri())
            .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
            .args([
                "orders",
                "trailing-stop",
                "--coin",
                coin,
                "--side",
                "sell",
                "--size",
                "1",
                "--retracement-pct",
                "5",
                "--testnet",
            ])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("perpetual markets only"));
    }
    assert_eq!(
        lookup_and_exchange_requests(&server).await,
        Vec::<String>::new(),
        "spot and outcome coins must fail before any lookup or exchange request"
    );
}

#[tokio::test]
async fn mainnet_json_without_yes_requires_confirmation_and_sends_nothing() {
    let env = IsolatedHome::new();
    let server = trading_server(trailing_stop_ok(OID)).await;

    let assert = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "trailing-stop",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--size",
            "0.01",
            "--retracement-pct",
            "5",
        ])
        .assert()
        .code(13);
    let error = json_stdout(&assert);
    assert_eq!(error["reason"], "confirmation_required");
    assert!(exchange_bodies(&server).await.is_empty());
}

#[tokio::test]
async fn mainnet_prompt_shows_every_consent_row_and_decline_sends_nothing() {
    let env = IsolatedHome::new();
    let server = trading_server(trailing_stop_ok(OID)).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("n\n")
        .args([
            "orders",
            "trailing-stop",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--size",
            "0.01",
            "--retracement-pct",
            "5",
            "--reduce-only",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Network: mainnet"))
        .stderr(predicate::str::contains("Signer: "))
        .stderr(predicate::str::contains("Coin: BTC"))
        .stderr(predicate::str::contains("Side: sell"))
        .stderr(predicate::str::contains("Size: 0.01"))
        .stderr(predicate::str::contains("Retracement: 5%"))
        .stderr(predicate::str::contains("starts tracking immediately"))
        .stderr(predicate::str::contains("Reduce-only: yes"))
        .stderr(predicate::str::contains(
            "Place this trailing stop on mainnet?",
        ))
        .stderr(predicate::str::contains(
            "trailing stop placement cancelled",
        ));
    assert!(exchange_bodies(&server).await.is_empty());
}

#[tokio::test]
async fn mainnet_prompt_acceptance_and_yes_both_place_the_stop() {
    let env = IsolatedHome::new();
    let server = trading_server(trailing_stop_ok(OID)).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("yes\n")
        .args([
            "orders",
            "trailing-stop",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--size",
            "0.01",
            "--retracement-px",
            "100",
            "--activation-price",
            "49000",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Activation: 49000"))
        .stderr(predicate::str::contains(
            "Retracement: 100 from the lowest mark price",
        ))
        .stdout(predicate::str::contains("77738308"));

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "trailing-stop",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--size",
            "0.01",
            "--retracement-pct",
            "5",
            "--yes",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Network: mainnet").not());
    assert_eq!(exchange_bodies(&server).await.len(), 2);
}

#[tokio::test]
async fn dry_run_makes_no_exchange_request_and_reports_the_plan() {
    let env = IsolatedHome::new();
    let server = trading_server(trailing_stop_ok(OID)).await;

    let assert = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "--dry-run",
            "orders",
            "trailing-stop",
            "--coin",
            "ETH",
            "--side",
            "sell",
            "--size",
            "0.5",
            "--retracement-pct",
            "5",
            "--reduce-only",
            "--activation-price",
            "3500",
            "--on-behalf-of",
            "0x00000000000000000000000000000000000000aa",
        ])
        .assert()
        .success();

    assert!(exchange_bodies(&server).await.is_empty());
    let envelope = json_stdout(&assert);
    assert_eq!(envelope["command"], "orders trailing-stop");
    assert_eq!(envelope["would_execute"], "trailing_stop");
    assert!(envelope["network"].is_string(), "{envelope}");
    let args = &envelope["args"];
    assert_eq!(args["coin"], "ETH");
    assert_eq!(args["asset_id"], 1);
    assert_eq!(args["side"], "sell");
    assert_eq!(args["size"], "0.5");
    assert_eq!(args["reduce_only"], true);
    assert_eq!(args["retracement"], json!({"pct": "5"}));
    assert_eq!(args["activation_px"], "3500");
    assert_eq!(
        args["on_behalf_of"],
        "0x00000000000000000000000000000000000000aa"
    );
}

#[tokio::test]
async fn pretty_dry_run_lists_the_arguments() {
    let env = IsolatedHome::new();
    let server = trading_server(trailing_stop_ok(OID)).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--dry-run",
            "orders",
            "trailing-stop",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--size",
            "0.01",
            "--retracement-px",
            "150",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("trailing_stop"))
        .stdout(predicate::str::contains("Retracement"));
    assert!(exchange_bodies(&server).await.is_empty());
}

#[tokio::test]
async fn exchange_rejection_is_labelled_untrusted_remote_data() {
    let env = IsolatedHome::new();
    let server = trading_server(json!({
        "status": "err",
        "response": "Trailing stops are disabled\u{1b}[31m for this asset"
    }))
    .await;

    let assert = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "trailing-stop",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--size",
            "0.01",
            "--retracement-pct",
            "5",
            "--testnet",
        ])
        .assert()
        .code(1);
    let error = json_stdout(&assert);
    let message = error["error"].as_str().unwrap();
    assert!(message.contains("[untrusted remote data]"), "{message}");
    assert!(message.contains("Trailing stops are disabled"), "{message}");
    assert!(!message.contains('\u{1b}'), "{message:?}");
    assert_eq!(exchange_bodies(&server).await.len(), 1, "never retried");
}

#[tokio::test]
async fn exchange_answering_ok_without_an_oid_is_an_unknown_outcome() {
    let env = IsolatedHome::new();
    let server = trading_server(json!({"status": "ok", "response": {"type": "default"}})).await;

    let assert = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "trailing-stop",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--size",
            "0.01",
            "--retracement-pct",
            "5",
            "--testnet",
        ])
        .assert()
        .code(12);
    assert_eq!(json_stdout(&assert)["reason"], "unknown_outcome");
}

#[test]
fn help_describes_the_mark_price_trigger_and_notes() {
    let env = IsolatedHome::new();
    env.command()
        .args(["orders", "trailing-stop", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Place a native trailing stop on a perp market",
        ))
        .stdout(predicate::str::contains("mark price"))
        .stdout(predicate::str::contains("market order"))
        .stdout(predicate::str::contains("--retracement-pct"))
        .stdout(predicate::str::contains("Notes:"))
        .stdout(predicate::str::contains("Dry-run: supported"));
}

#[tokio::test]
async fn open_orders_and_history_render_trailing_stop_markets() {
    let env = IsolatedHome::new();
    let server = MockServer::start().await;
    mount_common_public_endpoints(&server).await;
    let order = json!({
        "timestamp": 1_700_000_000_000_u64,
        "coin": "BTC",
        "side": "A",
        "limitPx": "0.0",
        "sz": "0.01",
        "oid": OID,
        "origSz": "0.01",
        "cloid": null,
        "orderType": "Trailing Stop Market",
        "tif": null,
        "reduceOnly": true,
        "isTrigger": true,
        "triggerPx": "0.0",
        "triggerCondition": "N/A",
        "isPositionTpsl": false,
    });
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(json!({"type": "frontendOpenOrders"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([order.clone()])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(json!({"type": "historicalOrders"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "order": order,
            "status": "open",
            "statusTimestamp": 1_700_000_000_001_u64
        }])))
        .mount(&server)
        .await;

    for subcommand in ["open", "history"] {
        let assert = env
            .command()
            .env(API_OVERRIDE_ENV, server.uri())
            .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
            .args(["--format", "json", "orders", subcommand, "--testnet"])
            .assert()
            .success();
        let rows = json_stdout(&assert);
        assert_eq!(rows[0]["order_type"], "TrailingStopMarket", "{subcommand}");
        assert_eq!(rows[0]["oid"], OID);
    }
}
