mod support;

use predicates::prelude::*;
use serde_json::Value;
use support::{
    API_OVERRIDE_ENV, IsolatedHome, PRIVATE_KEY_ENV, VALID_PRIVATE_KEY, expected_address,
};
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mock_order_server(oid: u64) -> MockServer {
    mock_order_server_with_leverage_type(oid, "cross").await
}

async fn mock_order_server_with_leverage_type(oid: u64, leverage_type: &str) -> MockServer {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "allMids"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "BTC": "50000",
            "ETH": "3000",
            "TICKY": "94720.33",
            "BLUR": "0.25",
            "BONK": "0.00001"
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "spotMeta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [],
            "tokens": [
                {
                    "name": "USDC",
                    "index": 0,
                    "tokenId": "0x00000000000000000000000000000000",
                    "szDecimals": 6,
                    "weiDecimals": 6,
                    "evmContract": null
                }
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "meta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [
                {
                    "name": "BTC",
                    "szDecimals": 5,
                    "maxLeverage": 50,
                    "onlyIsolated": false,
                    "marginMode": null,
                    "growthMode": "disabled"
                },
                {
                    "name": "ETH",
                    "szDecimals": 4,
                    "maxLeverage": 50,
                    "onlyIsolated": false,
                    "marginMode": null,
                    "growthMode": "disabled"
                },
                {
                    "name": "TICKY",
                    "szDecimals": 5,
                    "maxLeverage": 50,
                    "onlyIsolated": false,
                    "marginMode": null,
                    "growthMode": "disabled"
                },
                {
                    "name": "BLUR",
                    "szDecimals": 1,
                    "maxLeverage": 10,
                    "onlyIsolated": false,
                    "marginMode": null,
                    "growthMode": "disabled"
                },
                {
                    "name": "BONK",
                    "szDecimals": 0,
                    "maxLeverage": 5,
                    "onlyIsolated": false,
                    "marginMode": null,
                    "growthMode": "disabled"
                }
            ],
            "collateralToken": 0
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "perpDexs"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "clearinghouseState"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "marginSummary": {
                "accountValue": "10000",
                "totalNtlPos": "5000",
                "totalRawUsd": "10000",
                "totalMarginUsed": "1000"
            },
            "crossMarginSummary": {
                "accountValue": "10000",
                "totalNtlPos": "5000",
                "totalRawUsd": "10000",
                "totalMarginUsed": "1000"
            },
            "crossMaintenanceMarginUsed": "100",
            "withdrawable": "1000",
            "assetPositions": [
                {
                    "type": "oneWay",
                    "position": {
                        "coin": "BTC",
                        "szi": "0.1",
                        "leverage": {"type": leverage_type, "value": 5},
                        "entryPx": "50000",
                        "positionValue": "5100",
                        "unrealizedPnl": "100",
                        "returnOnEquity": "0.1",
                        "liquidationPx": null,
                        "marginUsed": "1000",
                        "maxLeverage": 50,
                        "cumFunding": {"allTime": "0", "sinceOpen": "0", "sinceChange": "0"}
                    }
                }
            ],
            "time": 1700000000000_u64
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": "order",
                "data": {
                    "statuses": [
                        {
                            "resting": {
                                "oid": oid,
                                "cloid": null
                            }
                        }
                    ]
                }
            }
        })))
        .mount(&server)
        .await;

    server
}

async fn mock_order_server_without_exchange() -> MockServer {
    let server = mock_order_server(0).await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    server
}

async fn mock_grouped_order_server() -> MockServer {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "allMids"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "BTC": "50000",
            "ETH": "3000"
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "spotMeta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [],
            "tokens": [
                {
                    "name": "USDC",
                    "index": 0,
                    "tokenId": "0x00000000000000000000000000000000",
                    "szDecimals": 6,
                    "weiDecimals": 6,
                    "evmContract": null
                }
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "meta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [
                {
                    "name": "ETH",
                    "szDecimals": 4,
                    "maxLeverage": 50,
                    "onlyIsolated": false,
                    "marginMode": null,
                    "growthMode": "disabled"
                }
            ],
            "collateralToken": 0
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "perpDexs"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "clearinghouseState"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "marginSummary": {
                "accountValue": "10000",
                "totalNtlPos": "5000",
                "totalRawUsd": "10000",
                "totalMarginUsed": "1000"
            },
            "crossMarginSummary": {
                "accountValue": "10000",
                "totalNtlPos": "5000",
                "totalRawUsd": "10000",
                "totalMarginUsed": "1000"
            },
            "crossMaintenanceMarginUsed": "100",
            "withdrawable": "1000",
            "assetPositions": [
                {
                    "type": "oneWay",
                    "position": {
                        "coin": "ETH",
                        "szi": "0.1",
                        "leverage": {"type": "cross", "value": 5},
                        "entryPx": "3000",
                        "positionValue": "300",
                        "unrealizedPnl": "10",
                        "returnOnEquity": "0.1",
                        "liquidationPx": null,
                        "marginUsed": "60",
                        "maxLeverage": 50,
                        "cumFunding": {"allTime": "0", "sinceOpen": "0", "sinceChange": "0"}
                    }
                }
            ],
            "time": 1700000000000_u64
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains("\"grouping\":\"normalTpsl\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": "order",
                "data": {
                    "statuses": [
                        {"resting": {"oid": 221, "cloid": null}},
                        {"resting": {"oid": 222, "cloid": null}},
                        {"resting": {"oid": 223, "cloid": null}}
                    ]
                }
            }
        })))
        .mount(&server)
        .await;

    server
}

async fn mock_batch_order_server() -> MockServer {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "allMids"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "BTC": "50000",
            "ETH": "3000"
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "spotMeta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [],
            "tokens": [
                {
                    "name": "USDC",
                    "index": 0,
                    "tokenId": "0x00000000000000000000000000000000",
                    "szDecimals": 6,
                    "weiDecimals": 6,
                    "evmContract": null
                }
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "meta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [
                {
                    "name": "BTC",
                    "szDecimals": 5,
                    "maxLeverage": 50,
                    "onlyIsolated": false,
                    "marginMode": null,
                    "growthMode": "disabled"
                },
                {
                    "name": "ETH",
                    "szDecimals": 4,
                    "maxLeverage": 50,
                    "onlyIsolated": false,
                    "marginMode": null,
                    "growthMode": "disabled"
                }
            ],
            "collateralToken": 0
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "perpDexs"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": "order",
                "data": {
                    "statuses": [
                        {"resting": {"oid": 331, "cloid": null}},
                        {"resting": {"oid": 332, "cloid": null}}
                    ]
                }
            }
        })))
        .mount(&server)
        .await;

    server
}

async fn exchange_order_body(server: &MockServer) -> Value {
    let requests = server.received_requests().await.unwrap();
    let exchange_request = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected /exchange request");
    serde_json::from_slice(&exchange_request.body).unwrap()
}

async fn clearinghouse_state_request_body(server: &MockServer) -> Value {
    let requests = server.received_requests().await.unwrap();
    requests
        .iter()
        .filter(|request| request.url.path() == "/info")
        .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
        .find(|body| body["type"] == "clearinghouseState")
        .expect("expected clearinghouseState request")
}

#[test]
fn orders_create_missing_limit_args_exits_2() {
    let env = IsolatedHome::new();
    env.command()
        .args(["orders", "create", "--coin", "BTC", "--side", "buy"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--price"))
        .stderr(predicate::str::contains("--size"));
}

#[test]
fn orders_create_limit_rejects_amount_flag() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--amount",
            "100",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--type limit"))
        .stderr(predicate::str::contains("remove --amount"))
        .stderr(predicate::str::contains("--type market --amount"));
}

#[test]
fn orders_create_limit_with_zero_amount_reports_incompatible_flag_first() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--amount",
            "0",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--type limit"))
        .stderr(predicate::str::contains("remove --amount"))
        .stderr(predicate::str::contains("amount must be greater than zero").not());
}

#[test]
fn orders_create_market_rejects_price_and_size_flags() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--amount",
            "100",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--type",
            "market",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--type market"))
        .stderr(predicate::str::contains("remove --price and --size"))
        .stderr(predicate::str::contains("--amount"));
}

#[test]
fn orders_create_market_rejects_trigger_price_with_trigger_guidance() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--amount",
            "100",
            "--trigger-price",
            "50000",
            "--type",
            "market",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--type market"))
        .stderr(predicate::str::contains("remove --trigger-price"))
        .stderr(predicate::str::contains("stop-loss/take-profit"))
        .stderr(predicate::str::contains("stop-limit/take-limit"));
}

#[test]
fn orders_create_market_with_zero_price_and_size_reports_incompatible_flags_first() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--amount",
            "100",
            "--price",
            "0",
            "--size",
            "0",
            "--type",
            "market",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--type market"))
        .stderr(predicate::str::contains("remove --price and --size"))
        .stderr(predicate::str::contains("price must be positive").not())
        .stderr(predicate::str::contains("size must be greater than zero").not());
}

#[test]
fn orders_create_stop_loss_rejects_amount_flag() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--price",
            "49000",
            "--size",
            "0.1",
            "--amount",
            "100",
            "--type",
            "stop-loss",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--type stop-loss"))
        .stderr(predicate::str::contains("remove --amount"))
        .stderr(predicate::str::contains("uses --trigger-price"))
        .stderr(predicate::str::contains("legacy --price"));
}

#[test]
fn orders_create_take_profit_rejects_amount_flag() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--price",
            "55000",
            "--size",
            "0.1",
            "--amount",
            "100",
            "--type",
            "take-profit",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--type take-profit"))
        .stderr(predicate::str::contains("remove --amount"))
        .stderr(predicate::str::contains("uses --trigger-price"))
        .stderr(predicate::str::contains("legacy --price"));
}

#[test]
fn orders_trigger_limit_stop_limit_requires_trigger_price() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--type",
            "stop-limit",
            "--price",
            "89500",
            "--size",
            "0.001",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--trigger-price"))
        .stderr(predicate::str::contains("limit trigger orders"));
}

#[test]
fn orders_create_stop_loss_rejects_ambiguous_trigger_and_price_flags() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--type",
            "stop-loss",
            "--trigger-price",
            "49000",
            "--price",
            "48500",
            "--size",
            "0.001",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--type stop-loss"))
        .stderr(predicate::str::contains("remove one price flag"))
        .stderr(predicate::str::contains("stop-limit/take-limit"));
}

#[tokio::test]
async fn orders_trigger_limit_dry_run_json_exposes_reduce_only_behavior() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "--dry-run",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--type",
            "stop-limit",
            "--trigger-price",
            "90000",
            "--price",
            "89500",
            "--size",
            "0.001",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["command"], "orders create");
    assert_eq!(json["would_execute"], "submit_order");
    assert_eq!(json["args"]["type"], "stop-limit");
    assert_eq!(json["args"]["asset_id"], 0);
    assert_eq!(json["args"]["resolved_asset"], "BTC");
    assert_eq!(json["args"]["limit_px"], "89500");
    assert_eq!(json["args"]["trigger_px"], "90000");
    assert_eq!(json["args"]["is_market"], false);
    assert_eq!(json["args"]["tpsl"], "sl");
    assert_eq!(json["args"]["reduce_only"], false);

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "--dry-run",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--type",
            "take-limit",
            "--trigger-price",
            "110000",
            "--price",
            "109500",
            "--size",
            "0.001",
            "--reduce-only",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["args"]["type"], "take-limit");
    assert_eq!(json["args"]["asset_id"], 0);
    assert_eq!(json["args"]["limit_px"], "109500");
    assert_eq!(json["args"]["trigger_px"], "110000");
    assert_eq!(json["args"]["is_market"], false);
    assert_eq!(json["args"]["tpsl"], "tp");
    assert_eq!(json["args"]["reduce_only"], true);
}

#[tokio::test]
async fn orders_create_dry_run_includes_builder_fee_params() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;
    let builder = "0x1111111111111111111111111111111111111111";

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "--dry-run",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "50000",
            "--size",
            "0.001",
            "--builder",
            builder,
            "--builder-fee-rate",
            "0.01%",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["args"]["builder"]["b"], builder);
    assert_eq!(json["args"]["builder"]["f"], 10);
    assert_eq!(json["args"]["builder_fee_rate"], "0.01%");
}

#[tokio::test]
async fn orders_create_live_builder_fee_enters_signed_action() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12360).await;
    let builder = "0x1111111111111111111111111111111111111111";

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "50000",
            "--size",
            "0.001",
            "--builder",
            builder,
            "--builder-fee-rate",
            "0.01%",
            "--testnet",
        ])
        .assert()
        .success();

    let body = exchange_order_body(&server).await;
    assert_eq!(body["action"]["type"], "order");
    assert_eq!(body["action"]["builder"]["b"], builder);
    assert_eq!(body["action"]["builder"]["f"], 10);
    assert_eq!(body["action"]["orders"][0]["a"], 0);
}

#[tokio::test]
async fn orders_create_live_builder_fee_lowercases_builder_address() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12361).await;
    let builder = "0x8c967E73E7B15087c42A10D344cFf4c96D877f1D";

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "50000",
            "--size",
            "0.001",
            "--builder",
            builder,
            "--builder-fee-rate",
            "0.01%",
            "--testnet",
        ])
        .assert()
        .success();

    let body = exchange_order_body(&server).await;
    assert_eq!(
        body["action"]["builder"]["b"],
        "0x8c967e73e7b15087c42a10d344cff4c96d877f1d"
    );
}

#[test]
fn orders_create_builder_fee_validation_fails_before_auth() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "50000",
            "--size",
            "0.001",
            "--builder",
            "0x0000000000000000000000000000000000000000",
            "--builder-fee-rate",
            "0.01%",
            "--testnet",
        ])
        .assert()
        .code(13)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "builder address cannot be the zero address",
        ));
}

#[tokio::test]
async fn orders_create_outcome_notation_dry_run_exposes_asset_encoding() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "--dry-run",
            "orders",
            "create",
            "--coin",
            "#10",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "0.5",
            "--size",
            "1",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["args"]["asset_id"], 100000010);
    assert_eq!(json["args"]["outcome"]["outcome"], 1);
    assert_eq!(json["args"]["outcome"]["side"], 0);
    assert_eq!(json["args"]["outcome"]["encoding"], 10);
}

#[tokio::test]
async fn orders_create_outcome_live_submits_after_auth() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12345).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "#10",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "0.5",
            "--size",
            "1",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("#10"));
}

#[tokio::test]
async fn orders_create_dry_run_includes_cloid() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "--dry-run",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "50000",
            "--size",
            "0.001",
            "--cloid",
            "0x1234567890abcdef1234567890abcdef",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["args"]["cloid"], "0x1234567890abcdef1234567890abcdef");
}

#[tokio::test]
async fn orders_scale_dry_run_json_includes_deterministic_legs() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "--dry-run",
            "orders",
            "scale",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--start-price",
            "80000",
            "--end-price",
            "90000",
            "--total-size",
            "0.005",
            "--orders",
            "5",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["command"], "orders scale");
    assert_eq!(json["would_execute"], "submit_order_batch");
    assert_eq!(json["args"]["order_count"], 5);
    let orders = json["args"]["orders"].as_array().unwrap();
    assert_eq!(orders.len(), 5);
    assert_eq!(orders[0]["leg_index"], 0);
    assert_eq!(orders[0]["coin"], "BTC");
    assert_eq!(orders[0]["price"], "80000");
    assert_eq!(orders[0]["size"], "0.001");
    assert_eq!(orders[4]["leg_index"], 4);
    assert_eq!(orders[4]["price"], "90000");
    assert_eq!(orders[4]["size"], "0.001");
}

#[tokio::test]
async fn orders_scale_dry_run_json_preserves_on_behalf_of_signing_context() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;
    let signer = expected_address(VALID_PRIVATE_KEY);
    let subaccount = "0x0000000000000000000000000000000000000000";

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "--dry-run",
            "orders",
            "scale",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--start-price",
            "80000",
            "--end-price",
            "90000",
            "--total-size",
            "0.005",
            "--orders",
            "5",
            "--on-behalf-of",
            subaccount,
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["args"]["on_behalf_of"], subaccount);
    assert_eq!(json["signer"], signer);
    assert_eq!(json["acting_as"], subaccount);
    assert_eq!(json["vault_address"], subaccount);
}

#[tokio::test]
async fn orders_batch_create_dry_run_reads_fixture() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "--dry-run",
            "orders",
            "batch-create",
            "--orders-file",
            "tests/fixtures/orders_batch_create.json",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["command"], "orders batch-create");
    assert_eq!(json["would_execute"], "submit_order_batch");
    let orders = json["args"]["orders"].as_array().unwrap();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0]["leg_index"], 0);
    assert_eq!(orders[0]["coin"], "BTC");
    assert_eq!(orders[0]["price"], "50000");
    assert_eq!(orders[0]["size"], "0.001");
    assert_eq!(orders[1]["leg_index"], 1);
    assert_eq!(orders[1]["coin"], "ETH");
    assert_eq!(orders[1]["reduce_only"], true);
    assert_eq!(
        json["args"]["orders_file"],
        "tests/fixtures/orders_batch_create.json"
    );
    assert!(json["args"]["on_behalf_of"].is_null());
}

#[tokio::test]
async fn orders_batch_create_malformed_file_exits_before_auth() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "orders",
            "batch-create",
            "--orders-file",
            "tests/fixtures/orders_batch_create_bad.json",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid orders file"))
        .stderr(predicate::str::contains("price"));
}

#[tokio::test]
async fn orders_trigger_limit_live_request_keeps_limit_and_trigger_prices_distinct() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12353).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--type",
            "stop-limit",
            "--trigger-price",
            "90000",
            "--price",
            "89500",
            "--size",
            "0.001",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("12353"));

    let requests = server.received_requests().await.unwrap();
    let exchange_request = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected order exchange request");
    let body: Value = serde_json::from_slice(&exchange_request.body).unwrap();
    let order = &body["action"]["orders"][0];
    assert_eq!(order["p"], "89500");
    assert_eq!(order["t"]["trigger"]["isMarket"], false);
    assert_eq!(order["t"]["trigger"]["triggerPx"], "90000");
    assert_eq!(order["t"]["trigger"]["tpsl"], "sl");
}

#[tokio::test]
async fn orders_create_live_request_sends_cloid_in_c() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12354).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "50000",
            "--size",
            "0.001",
            "--cloid",
            "0x1234567890abcdef1234567890abcdef",
            "--testnet",
        ])
        .assert()
        .success();

    let body = exchange_order_body(&server).await;
    assert_eq!(
        body["action"]["orders"][0]["c"],
        "0x1234567890abcdef1234567890abcdef"
    );
}

#[tokio::test]
async fn orders_create_invalid_cloid_exits_before_auth() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "50000",
            "--size",
            "0.001",
            "--cloid",
            "0xabc",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("CLOID must be exactly 16 bytes"))
        .stderr(predicate::str::contains("Authentication required").not());
}

#[tokio::test]
async fn orders_trigger_limit_mainnet_prompt_includes_trigger_price() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("n\n")
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--type",
            "stop-limit",
            "--trigger-price",
            "90000",
            "--price",
            "89500",
            "--size",
            "0.001",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("trigger price 90000"))
        .stderr(predicate::str::contains("at price 89500"));
}

#[tokio::test]
async fn orders_create_negative_price_exits_2() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12345).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "-50000",
            "--size",
            "0.1",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("price must be positive"));
}

#[test]
fn orders_create_table_validation_error_contains_no_ansi() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "--format",
            "table",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "-50000",
            "--size",
            "0.1",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "Error: Configuration error: price must be positive",
        ))
        .stderr(predicate::str::contains("\u{1b}[").not());
}

#[tokio::test]
async fn orders_create_zero_size_exits_2() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12345).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("size must be greater than zero"));
}

#[tokio::test]
async fn orders_create_invalid_coin_exits_13_with_suggestion() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12345).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "orders",
            "create",
            "--coin",
            "BT",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--testnet",
        ])
        .assert()
        .code(13)
        .stderr(predicate::str::contains("Did you mean"))
        .stderr(predicate::str::contains("BTC"));
}

#[tokio::test]
async fn orders_create_invalid_coin_placeholder_exits_13_with_suggestion() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12345).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "orders",
            "create",
            "--coin",
            "INVALIDCOIN",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--testnet",
        ])
        .assert()
        .code(13)
        .stderr(predicate::str::contains("\"INVALIDCOIN\" not found"))
        .stderr(predicate::str::contains("Did you mean"))
        .stderr(predicate::str::contains("BTC"));
}

#[tokio::test]
async fn orders_create_invalid_coin_without_rankable_match_has_clean_not_found_error() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12345).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "orders",
            "create",
            "--coin",
            "ZZZZZZZZ",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--testnet",
        ])
        .assert()
        .code(13)
        .stderr(predicate::str::contains("\"ZZZZZZZZ\" not found."))
        .stderr(predicate::str::contains("Did you mean").not());
}

#[tokio::test]
async fn orders_create_without_wallet_exits_10_after_validation() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12345).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--testnet",
        ])
        .assert()
        .code(10)
        .stderr(predicate::str::contains("Authentication required"));
}

#[tokio::test]
async fn orders_create_limit_order_outputs_order_id() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12345).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--tif",
            "ioc",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Order ID"))
        .stdout(predicate::str::contains("12345"));
}

#[tokio::test]
async fn orders_create_normal_tpsl_outputs_per_leg_statuses() {
    let env = IsolatedHome::new();
    let server = mock_grouped_order_server().await;
    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "create",
            "--coin",
            "ETH",
            "--side",
            "buy",
            "--price",
            "2000",
            "--size",
            "0.1",
            "--take-profit",
            "2200",
            "--stop-loss",
            "1900",
            "--grouping",
            "normal-tpsl",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let rows: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["grouping"], "normal-tpsl");
    assert_eq!(rows[0]["grouping_wire"], "normalTpsl");
    assert_eq!(rows[0]["leg"], "parent");
    assert_eq!(rows[0]["order_id"], 221);
    assert_eq!(rows[1]["leg"], "take_profit");
    assert_eq!(rows[1]["side"], "sell");
    assert_eq!(rows[1]["reduce_only"], true);
    assert_eq!(rows[2]["leg"], "stop_loss");
    assert_eq!(rows[2]["order_id"], 223);
}

#[tokio::test]
async fn orders_batch_create_live_request_sends_single_order_action_with_all_legs() {
    let env = IsolatedHome::new();
    let server = mock_batch_order_server().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "batch-create",
            "--orders-file",
            "tests/fixtures/orders_batch_create.json",
            "--testnet",
        ])
        .assert()
        .success();

    let body = exchange_order_body(&server).await;
    assert_eq!(body["action"]["grouping"], "na");
    let orders = body["action"]["orders"].as_array().unwrap();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0]["a"], 0);
    assert_eq!(orders[0]["b"], true);
    assert_eq!(orders[0]["p"], "50000");
    assert_eq!(orders[0]["s"], "0.001");
    assert_eq!(orders[0]["r"], false);
    assert_eq!(orders[1]["a"], 1);
    assert_eq!(orders[1]["b"], false);
    assert_eq!(orders[1]["p"], "3000");
    assert_eq!(orders[1]["s"], "0.01");
    assert_eq!(orders[1]["r"], true);
}

#[tokio::test]
async fn orders_create_limit_reduce_only_submits_reduce_only_wire_order() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12345).await;
    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--type",
            "limit",
            "--price",
            "90000",
            "--size",
            "0.001",
            "--reduce-only",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json[0]["reduce_only"], true);

    let body = exchange_order_body(&server).await;
    assert_eq!(body["action"]["orders"][0]["r"], true);
}

#[tokio::test]
async fn orders_create_market_reduce_only_submits_reduce_only_wire_order() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12346).await;
    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--type",
            "market",
            "--amount",
            "50",
            "--reduce-only",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json[0]["reduce_only"], true);

    let body = exchange_order_body(&server).await;
    assert_eq!(body["action"]["orders"][0]["r"], true);
}

#[tokio::test]
async fn orders_create_rejects_margin_mode_mismatch_before_submission() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12347).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "50000",
            "--size",
            "0.001",
            "--margin-mode",
            "isolated",
            "--testnet",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requested --margin-mode isolated"))
        .stderr(predicate::str::contains(
            "current BTC perp position is cross",
        ))
        .stderr(predicate::str::contains(
            "positions update-leverage --coin BTC --leverage <N> --isolated",
        ));
}

#[tokio::test]
async fn orders_create_omitted_margin_mode_does_not_block_existing_isolated_position() {
    let env = IsolatedHome::new();
    let server = mock_order_server_with_leverage_type(12348, "isolated").await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "50000",
            "--size",
            "0.001",
            "--testnet",
        ])
        .assert()
        .success();
}

#[tokio::test]
async fn orders_create_validates_margin_mode_against_on_behalf_of_account() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12349).await;
    let subaccount = "0x0000000000000000000000000000000000000001";

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "50000",
            "--size",
            "0.001",
            "--margin-mode",
            "cross",
            "--on-behalf-of",
            subaccount,
            "--testnet",
        ])
        .assert()
        .success();

    let body = clearinghouse_state_request_body(&server).await;
    assert_eq!(body["user"], subaccount);
}

#[tokio::test]
async fn orders_create_mainnet_prompts_and_aborts_without_confirmation() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("n\n")
        .args([
            "orders", "create", "--coin", "BTC", "--side", "buy", "--price", "50000", "--size",
            "0.1",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "Mainnet order confirmation required",
        ))
        .stderr(predicate::str::contains("not reduce-only"))
        .stderr(predicate::str::contains("Slippage warning").not())
        .stderr(predicate::str::contains("order placement cancelled"));
}

#[tokio::test]
async fn orders_create_mainnet_reduce_only_prompt_includes_reduce_only_true() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("n\n")
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--reduce-only",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "Mainnet order confirmation required",
        ))
        .stderr(predicate::str::contains("reduce-only"))
        .stderr(predicate::str::contains("not reduce-only").not())
        .stderr(predicate::str::contains("order placement cancelled"));
}

#[tokio::test]
async fn orders_create_mainnet_market_prompt_includes_slippage_warning_before_abort() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("n\n")
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--amount",
            "100",
            "--type",
            "market",
            "--max-slippage-bps",
            "250",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "Mainnet order confirmation required",
        ))
        .stderr(predicate::str::contains("Slippage warning"))
        .stderr(predicate::str::contains("250 bps"))
        .stderr(predicate::str::contains("order placement cancelled"));
}

#[tokio::test]
async fn orders_create_mainnet_confirmation_abort_is_json_error() {
    let env = IsolatedHome::new();
    let server = mock_order_server_without_exchange().await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("n\n")
        .args([
            "--format", "json", "orders", "create", "--coin", "BTC", "--side", "buy", "--price",
            "50000", "--size", "0.1",
        ])
        .assert()
        .code(13)
        .stdout(predicate::str::contains(
            "requires confirmation in machine-readable contexts",
        ))
        .stdout(predicate::str::contains("\"error\""))
        .stderr(predicate::str::is_empty());
}

#[tokio::test]
async fn orders_create_mainnet_prompt_acceptance_places_order() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12350).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("yes\n")
        .args([
            "orders", "create", "--coin", "BTC", "--side", "buy", "--price", "50000", "--size",
            "0.1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("12350"))
        .stderr(predicate::str::contains(
            "Mainnet order confirmation required",
        ));
}

#[tokio::test]
async fn orders_create_mainnet_yes_bypasses_confirmation_prompt() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12348).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders", "create", "--coin", "BTC", "--side", "buy", "--price", "50000", "--size",
            "0.1", "--yes",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("12348"))
        .stderr(predicate::str::contains("Mainnet order confirmation required").not());
}

#[tokio::test]
async fn orders_create_testnet_does_not_prompt_for_mainnet_confirmation() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12349).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("12349"))
        .stderr(predicate::str::contains("Mainnet order confirmation required").not());
}

#[tokio::test]
async fn orders_create_market_order_accepts_amount_and_warns_about_configured_slippage() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12346).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--amount",
            "100",
            "--type",
            "market",
            "--max-slippage-bps",
            "250",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Order ID"))
        .stdout(predicate::str::contains("12346"))
        .stdout(predicate::str::contains("Slippage warning"))
        .stdout(predicate::str::contains("250 bps"))
        .stdout(predicate::str::contains("51250"));
}

#[tokio::test]
async fn orders_create_market_order_rounds_slippage_price_to_tick() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12347).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "TICKY",
            "--side",
            "buy",
            "--amount",
            "11",
            "--type",
            "market",
            "--max-slippage-bps",
            "200",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("12347"))
        .stdout(predicate::str::contains("TICKY"))
        .stdout(predicate::str::contains("96615"))
        .stdout(predicate::str::contains("96614.7366").not());
}

#[tokio::test]
async fn orders_create_market_slippage_warning_uses_yellow_in_pretty_only() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12352).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--amount",
            "100",
            "--type",
            "market",
            "--max-slippage-bps",
            "250",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\u{1b}[33mSlippage warning: market order uses a 250 bps",
        ))
        .stdout(predicate::str::contains("\u{1b}[33mfalse\u{1b}[0m").not());

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "table",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--amount",
            "100",
            "--type",
            "market",
            "--max-slippage-bps",
            "250",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Slippage warning"))
        .stdout(predicate::str::contains("\u{1b}[").not());

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--amount",
            "100",
            "--type",
            "market",
            "--max-slippage-bps",
            "250",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Slippage warning"))
        .stdout(predicate::str::contains("\u{1b}[").not());
}

#[tokio::test]
async fn orders_create_sell_market_order_uses_configured_slippage_bound() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12351).await;
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--amount",
            "100",
            "--type",
            "market",
            "--max-slippage-bps",
            "250",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("12351"))
        .stdout(predicate::str::contains("250 bps"))
        .stdout(predicate::str::contains("48750"));
}

#[test]
fn orders_create_market_order_rejects_out_of_bounds_slippage() {
    let env = IsolatedHome::new();
    env.command()
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--amount",
            "100",
            "--type",
            "market",
            "--max-slippage-bps",
            "0",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--max-slippage-bps"));
}

#[tokio::test]
async fn orders_create_stop_loss_and_take_profit_orders_succeed() {
    let env = IsolatedHome::new();
    let server = mock_order_server(12347).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--price",
            "49000",
            "--size",
            "0.1",
            "--type",
            "stop-loss",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("stop-loss"))
        .stdout(predicate::str::contains("12347"));

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "sell",
            "--price",
            "55000",
            "--size",
            "0.1",
            "--type",
            "take-profit",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("take-profit"))
        .stdout(predicate::str::contains("12347"));
}
