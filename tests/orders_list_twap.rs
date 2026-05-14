mod support;

use predicates::prelude::*;
use support::{
    API_OVERRIDE_ENV, IsolatedHome, PRIVATE_KEY_ENV, VALID_PRIVATE_KEY, fixture_spot_meta_usdc_only,
};
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn basic_order(coin: &str, oid: u64) -> serde_json::Value {
    serde_json::json!({
        "timestamp": 1700000000000_u64,
        "coin": coin,
        "side": "B",
        "limitPx": "50000",
        "sz": "0.1",
        "oid": oid,
        "origSz": "0.1",
        "cloid": null,
        "orderType": "Limit",
        "tif": "Gtc",
        "reduceOnly": false
    })
}

fn historical_order_update(coin: &str, oid: u64) -> serde_json::Value {
    serde_json::json!({
        "order": basic_order(coin, oid),
        "status": "filled",
        "statusTimestamp": 1700000000001_u64
    })
}

async fn mock_orders_server(
    open_orders: Vec<serde_json::Value>,
    historical_orders: Vec<serde_json::Value>,
) -> MockServer {
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
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture_spot_meta_usdc_only()))
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
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "frontendOpenOrders"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(open_orders))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "historicalOrders"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(historical_orders))
        .mount(&server)
        .await;

    server
}

async fn mock_trading_server(exchange_response: serde_json::Value) -> MockServer {
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
            "tokens": []
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
        .respond_with(ResponseTemplate::new(200).set_body_json(exchange_response))
        .mount(&server)
        .await;

    server
}

async fn mock_twap_server_without_exchange() -> MockServer {
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
            "tokens": []
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
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    server
}

#[tokio::test]
async fn orders_open_shows_empty_state() {
    let env = IsolatedHome::new();
    let server = mock_orders_server(vec![], vec![]).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "open"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No open orders"));
}

#[tokio::test]
async fn orders_open_pretty_empty_state_uses_theme_colors() {
    let env = IsolatedHome::new();
    let server = mock_orders_server(vec![], vec![]).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "open"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\u{1b}[36mMessage\u{1b}[0m"))
        .stdout(predicate::str::contains(
            "\u{1b}[90mNo open orders\u{1b}[0m",
        ));
}

#[tokio::test]
async fn orders_open_table_and_json_empty_state_are_uncolored() {
    let env = IsolatedHome::new();
    let server = mock_orders_server(vec![], vec![]).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["--format", "table", "orders", "open"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No open orders"))
        .stdout(predicate::str::contains("\u{1b}[").not());

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["--format", "json", "orders", "open"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[]"))
        .stdout(predicate::str::contains("\u{1b}[").not());
}

#[tokio::test]
async fn orders_open_shows_open_orders() {
    let env = IsolatedHome::new();
    let server = mock_orders_server(vec![basic_order("BTC", 12345)], vec![]).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "open"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stdout(predicate::str::contains("12345"));
}

#[tokio::test]
async fn orders_open_selects_coin_price_and_size_json() {
    let env = IsolatedHome::new();
    let server = mock_orders_server(vec![basic_order("BTC", 12345)], vec![]).await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--testnet",
            "--format",
            "json",
            "--select",
            "coin,price,size",
            "orders",
            "open",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let first = json
        .as_array()
        .unwrap()
        .first()
        .unwrap()
        .as_object()
        .unwrap();

    assert_eq!(
        first.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["coin", "price", "size"]
    );
    assert_eq!(first["coin"], "BTC");
    assert_eq!(first["price"], "50000");
    assert_eq!(first["size"], "0.1");
}

#[tokio::test]
async fn orders_history_shows_historical_orders() {
    let env = IsolatedHome::new();
    let server = mock_orders_server(vec![], vec![basic_order("ETH", 54321)]).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "history"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ETH"))
        .stdout(predicate::str::contains("54321"));
}

#[tokio::test]
async fn orders_history_accepts_wrapped_live_order_updates() {
    let env = IsolatedHome::new();
    let server = mock_orders_server(vec![], vec![historical_order_update("ETH", 54321)]).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "history"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ETH"))
        .stdout(predicate::str::contains("54321"));
}

#[tokio::test]
async fn orders_twap_create_outputs_twap_id() {
    let env = IsolatedHome::new();
    let server = mock_trading_server(serde_json::json!({
        "status": "ok",
        "response": {
            "type": "twapOrder",
            "data": {
                "status": {
                    "running": {
                        "twapId": 77738308
                    }
                }
            }
        }
    }))
    .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains("\"type\":\"twapOrder\""))
        .and(body_string_contains("\"m\":60"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": "twapOrder",
                "data": {
                    "status": {
                        "running": {
                            "twapId": 77738308
                        }
                    }
                }
            }
        })))
        .mount(&server)
        .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "twap-create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--size",
            "1.0",
            "--duration",
            "3600",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("77738308"))
        .stdout(predicate::str::contains("running"));
}

#[tokio::test]
async fn orders_twap_create_normalizes_size_and_reports_matching_units() {
    let env = IsolatedHome::new();
    let server = mock_trading_server(serde_json::json!({
        "status": "ok",
        "response": {
            "type": "twapOrder",
            "data": {
                "status": {
                    "running": {
                        "twapId": 77738311
                    }
                }
            }
        }
    }))
    .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains("\"type\":\"twapOrder\""))
        .and(body_string_contains("\"m\":5"))
        .and(body_string_contains("\"s\":\"0.0007\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": "twapOrder",
                "data": {
                    "status": {
                        "running": {
                            "twapId": 77738311
                        }
                    }
                }
            }
        })))
        .mount(&server)
        .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "twap-create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--size",
            "0.00070",
            "--duration",
            "300",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("77738311"))
        .stdout(predicate::str::contains("300"))
        .stdout(predicate::str::contains("5"));

    let requests = server.received_requests().await.unwrap();
    let exchange_request = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected twap exchange request");
    let body: serde_json::Value = serde_json::from_slice(&exchange_request.body).unwrap();
    assert_eq!(body["action"]["twap"]["s"], "0.0007");
}

#[tokio::test]
async fn orders_twap_create_rejects_duration_less_than_five_minutes() {
    let env = IsolatedHome::new();
    let server = mock_twap_server_without_exchange().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "twap-create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--size",
            "1.0",
            "--duration",
            "299",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "TWAP duration must be at least 300 seconds",
        ))
        .stderr(predicate::str::contains("whole-minute"));
}

#[tokio::test]
async fn orders_twap_create_rejects_non_minute_duration() {
    let env = IsolatedHome::new();
    let server = mock_twap_server_without_exchange().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "twap-create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--size",
            "1.0",
            "--duration",
            "301",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "TWAP duration must be a whole-minute value in seconds",
        ))
        .stderr(predicate::str::contains("--duration 300"))
        .stderr(predicate::str::contains("--duration 600"));
}

#[test]
fn orders_twap_create_help_documents_whole_minute_duration() {
    let env = IsolatedHome::new();

    env.command()
        .args(["orders", "twap-create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("whole-minute value"))
        .stdout(predicate::str::contains("300, 600"));
}

#[tokio::test]
async fn orders_twap_create_mainnet_prompts_and_aborts_without_submission() {
    let env = IsolatedHome::new();
    let server = mock_twap_server_without_exchange().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("n\n")
        .args([
            "orders",
            "twap-create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--size",
            "1.0",
            "--duration",
            "3600",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "Mainnet TWAP confirmation required",
        ))
        .stderr(predicate::str::contains("BTC"))
        .stderr(predicate::str::contains("buy"))
        .stderr(predicate::str::contains("1.0"))
        .stderr(predicate::str::contains("3600"))
        .stderr(predicate::str::contains("mainnet"))
        .stderr(predicate::str::contains("TWAP creation cancelled"));
}

#[tokio::test]
async fn orders_twap_create_mainnet_yes_bypasses_confirmation_prompt() {
    let env = IsolatedHome::new();
    let server = mock_trading_server(serde_json::json!({
        "status": "ok",
        "response": {
            "type": "twapOrder",
            "data": {
                "status": {
                    "running": {
                        "twapId": 77738309
                    }
                }
            }
        }
    }))
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "twap-create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--size",
            "1.0",
            "--duration",
            "3600",
            "--yes",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("77738309"))
        .stderr(predicate::str::contains("Mainnet TWAP confirmation required").not());
}

#[tokio::test]
async fn orders_twap_create_testnet_remains_non_interactive() {
    let env = IsolatedHome::new();
    let server = mock_trading_server(serde_json::json!({
        "status": "ok",
        "response": {
            "type": "twapOrder",
            "data": {
                "status": {
                    "running": {
                        "twapId": 77738310
                    }
                }
            }
        }
    }))
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "twap-create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--size",
            "1.0",
            "--duration",
            "3600",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("77738310"))
        .stderr(predicate::str::contains("Mainnet TWAP confirmation required").not());
}

#[tokio::test]
async fn orders_twap_cancel_outputs_confirmation() {
    let env = IsolatedHome::new();
    let server = mock_trading_server(serde_json::json!({
        "status": "ok",
        "response": {
            "type": "twapCancel",
            "data": {
                "status": "success"
            }
        }
    }))
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "twap-cancel", "42", "--coin", "BTC", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("42"))
        .stdout(predicate::str::contains("cancelled"));
}

#[tokio::test]
async fn orders_twap_cancel_requires_coin() {
    let env = IsolatedHome::new();

    env.command()
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "twap-cancel", "42", "--testnet"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--coin"))
        .stderr(predicate::str::contains("required"));
}

#[tokio::test]
async fn twap_cancel_with_non_btc_coin() {
    let env = IsolatedHome::new();
    let server = mock_trading_server(serde_json::json!({
        "status": "ok",
        "response": {
            "type": "twapCancel",
            "data": {
                "status": "success"
            }
        }
    }))
    .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains("\"type\":\"twapCancel\""))
        .and(body_string_contains("\"a\":1"))
        .and(body_string_contains("\"t\":42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": "twapCancel",
                "data": {
                    "status": "success"
                }
            }
        })))
        .with_priority(1)
        .expect(1)
        .named("ETH twapCancel exchange request")
        .mount(&server)
        .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "twap-cancel", "42", "--coin", "ETH", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ETH"))
        .stdout(predicate::str::contains("42"))
        .stdout(predicate::str::contains("cancelled"));

    let requests = server.received_requests().await.unwrap();
    let exchange_requests: Vec<_> = requests
        .iter()
        .filter(|request| request.url.path() == "/exchange")
        .collect();
    assert_eq!(
        exchange_requests.len(),
        1,
        "expected exactly one /exchange request"
    );

    let body: serde_json::Value = serde_json::from_slice(&exchange_requests[0].body).unwrap();
    assert_eq!(body["action"]["type"], "twapCancel");
    assert_eq!(body["action"]["a"], 1);
    assert_eq!(body["action"]["t"], 42);
}

#[test]
fn orders_twap_cancel_help_shows_required_coin() {
    let env = IsolatedHome::new();

    env.command()
        .args(["orders", "twap-cancel", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--coin <COIN>"))
        .stdout(predicate::str::contains("Perpetual coin"));
}

#[tokio::test]
async fn orders_schedule_cancel_outputs_scheduled_time() {
    let env = IsolatedHome::new();
    let server = mock_trading_server(serde_json::json!({
        "status": "ok",
        "response": {
            "type": "default"
        }
    }))
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "schedule-cancel", "--in", "5m", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("scheduled"))
        .stdout(predicate::str::contains("Scheduled Time"));
}

#[tokio::test]
async fn orders_schedule_cancel_clear_outputs_cleared_status() {
    let env = IsolatedHome::new();
    let server = mock_trading_server(serde_json::json!({
        "status": "ok",
        "response": {
            "type": "default"
        }
    }))
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "schedule-cancel", "--clear", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cleared"));
}

#[test]
fn orders_twap_create_missing_params_exits_2() {
    let env = IsolatedHome::new();
    env.command()
        .args(["orders", "twap-create", "--coin", "BTC", "--side", "buy"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--size"))
        .stderr(predicate::str::contains("--duration"));
}
