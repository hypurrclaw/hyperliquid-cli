mod support;

use predicates::prelude::*;
use support::{API_OVERRIDE_ENV, IsolatedHome, PRIVATE_KEY_ENV, VALID_PRIVATE_KEY};
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TEST_CLOID: &str = "0x0000000000000000000000000000abcd";

fn basic_order(coin: &str, oid: u64, cloid: Option<&str>) -> serde_json::Value {
    basic_order_with_type(coin, oid, cloid, "Limit")
}

fn basic_order_with_type(
    coin: &str,
    oid: u64,
    cloid: Option<&str>,
    order_type: &str,
) -> serde_json::Value {
    serde_json::json!({
        "timestamp": 1700000000000_u64,
        "coin": coin,
        "side": "B",
        "limitPx": "50000",
        "sz": "0.1",
        "oid": oid,
        "origSz": "0.1",
        "cloid": cloid,
        "orderType": order_type,
        "tif": "Gtc",
        "reduceOnly": false
    })
}

fn spot_meta_with_hype_pair() -> serde_json::Value {
    serde_json::json!({
        "universe": [
            {
                "name": "HYPE/USDC",
                "index": 1035,
                "tokens": [1105, 0]
            }
        ],
        "tokens": [
            {
                "name": "USDC",
                "index": 0,
                "tokenId": "0x00000000000000000000000000000000",
                "szDecimals": 6,
                "weiDecimals": 6,
                "evmContract": null
            },
            {
                "name": "HYPE",
                "index": 1105,
                "tokenId": "0x00000000000000000000000000000001",
                "szDecimals": 5,
                "weiDecimals": 18,
                "evmContract": null
            }
        ]
    })
}

async fn mock_order_management_server(
    status_order: Option<serde_json::Value>,
    open_orders: Vec<serde_json::Value>,
    exchange_statuses: Vec<serde_json::Value>,
    exchange_response_type: &str,
) -> MockServer {
    mock_order_management_server_with_spot_meta(
        status_order,
        open_orders,
        exchange_statuses,
        exchange_response_type,
        serde_json::json!({
            "universe": [],
            "tokens": []
        }),
    )
    .await
}

async fn mock_order_management_server_with_spot_meta(
    status_order: Option<serde_json::Value>,
    open_orders: Vec<serde_json::Value>,
    exchange_statuses: Vec<serde_json::Value>,
    exchange_response_type: &str,
    spot_meta: serde_json::Value,
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
        .respond_with(ResponseTemplate::new(200).set_body_json(spot_meta))
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

    let order_status_response = match status_order {
        Some(order) => serde_json::json!({
            "status": "order",
            "order": {
                "status": "open",
                "statusTimestamp": 1700000000001_u64,
                "order": order
            }
        }),
        None => serde_json::json!({"status": "unknownOid"}),
    };
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "orderStatus"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(order_status_response))
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
        .and(path("/exchange"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": exchange_response_type,
                "data": {
                    "statuses": exchange_statuses
                }
            }
        })))
        .mount(&server)
        .await;

    server
}

#[tokio::test]
async fn orders_cancel_by_oid_outputs_confirmation() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        Some(basic_order("BTC", 12345, None)),
        vec![],
        vec![serde_json::json!("success")],
        "cancel",
    )
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "cancel", "12345", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cancelled"))
        .stdout(predicate::str::contains("12345"));
}

#[tokio::test]
async fn orders_cancel_by_cloid_outputs_confirmation() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        Some(basic_order("BTC", 12346, Some(TEST_CLOID))),
        vec![],
        vec![serde_json::json!("success")],
        "cancel",
    )
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "cancel", "--cloid", TEST_CLOID, "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cancelled"))
        .stdout(predicate::str::contains(TEST_CLOID));
}

#[tokio::test]
async fn orders_cancel_outcome_order_uses_encoded_asset_id() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        Some(basic_order("#10", 12347, None)),
        vec![],
        vec![serde_json::json!("success")],
        "cancel",
    )
    .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains("\"a\":100000010"))
        .and(body_string_contains("\"o\":12347"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": "cancel",
                "data": {
                    "statuses": ["success"]
                }
            }
        })))
        .mount(&server)
        .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "cancel", "12347", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#10"))
        .stdout(predicate::str::contains("12347"));
}

#[tokio::test]
async fn orders_cancel_unknown_order_exits_with_unknown_order_error() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(None, vec![], vec![], "cancel").await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "cancel", "999999999", "--testnet"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown order"))
        .stderr(predicate::str::contains("999999999"));
}

#[tokio::test]
async fn orders_cancel_spot_internal_asset_id_uses_numeric_asset() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server_with_spot_meta(
        Some(basic_order("@1035", 223344, None)),
        vec![],
        vec![serde_json::json!("success")],
        "cancel",
        spot_meta_with_hype_pair(),
    )
    .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains("\"a\":11035"))
        .and(body_string_contains("\"o\":223344"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": "cancel",
                "data": {
                    "statuses": ["success"]
                }
            }
        })))
        .mount(&server)
        .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "cancel", "223344", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HYPE/USDC"))
        .stdout(predicate::str::contains("223344"));
}

#[tokio::test]
async fn orders_open_normalizes_spot_internal_asset_ids_to_symbols() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server_with_spot_meta(
        None,
        vec![basic_order("@1035", 223344, None)],
        vec![],
        "cancel",
        spot_meta_with_hype_pair(),
    )
    .await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["--format", "json", "orders", "open", "--testnet"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let rows = json.as_array().unwrap();
    assert_eq!(rows[0]["coin"], "HYPE/USDC");
}

#[tokio::test]
async fn orders_cancel_all_prompts_and_aborts_on_no() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        None,
        vec![basic_order("BTC", 111, None)],
        vec![serde_json::json!("success")],
        "cancel",
    )
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("n\n")
        .args(["orders", "cancel-all", "--testnet"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Cancel all orders? [y/N]"))
        .stderr(predicate::str::contains("cancel-all aborted"));
}

#[tokio::test]
async fn orders_cancel_all_pretty_prompt_uses_yellow_warning() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        None,
        vec![basic_order("BTC", 111, None)],
        vec![serde_json::json!("success")],
        "cancel",
    )
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("n\n")
        .args(["orders", "cancel-all", "--testnet"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "\u{1b}[33mCancel all orders? [y/N] \u{1b}[0m",
        ));
}

#[tokio::test]
async fn orders_cancel_all_table_prompt_stays_uncolored() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        None,
        vec![basic_order("BTC", 111, None)],
        vec![serde_json::json!("success")],
        "cancel",
    )
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("n\n")
        .args(["--format", "table", "orders", "cancel-all", "--testnet"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Cancel all orders? [y/N]"))
        .stderr(predicate::str::contains("\u{1b}[33mCancel all orders").not());
}

#[tokio::test]
async fn orders_cancel_all_yes_skips_prompt_and_shows_count() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        None,
        vec![basic_order("BTC", 111, None), basic_order("ETH", 222, None)],
        vec![serde_json::json!("success"), serde_json::json!("success")],
        "cancel",
    )
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "cancel-all", "-y", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cancelled Orders"))
        .stdout(predicate::str::contains("2"))
        .stderr(predicate::str::contains("Cancel all orders? [y/N]").not());
}

#[tokio::test]
async fn orders_cancel_all_coin_filter_cancels_only_matching_coin() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        None,
        vec![basic_order("BTC", 111, None), basic_order("ETH", 222, None)],
        vec![serde_json::json!("success")],
        "cancel",
    )
    .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains("\"o\":111"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": "cancel",
                "data": {
                    "statuses": ["success"]
                }
            }
        })))
        .mount(&server)
        .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "cancel-all", "--coin", "BTC", "-y", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stdout(predicate::str::contains("1"));
}

#[tokio::test]
async fn orders_cancel_all_invalid_coin_exits_with_asset_error() {
    let env = IsolatedHome::new();
    let server =
        mock_order_management_server(None, vec![basic_order("BTC", 111, None)], vec![], "cancel")
            .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "cancel-all", "--coin", "BTX", "-y", "--testnet"])
        .assert()
        .code(13)
        .stderr(predicate::str::contains("BTX"))
        .stderr(predicate::str::contains("Did you mean"));
}

#[tokio::test]
async fn orders_cancel_all_outcome_filter_normalizes_yes_notation() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        None,
        vec![basic_order("#10", 333, None), basic_order("#11", 444, None)],
        vec![serde_json::json!("success")],
        "cancel",
    )
    .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains("\"a\":100000010"))
        .and(body_string_contains("\"o\":333"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": "cancel",
                "data": {
                    "statuses": ["success"]
                }
            }
        })))
        .mount(&server)
        .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "cancel-all", "--coin", "+10", "-y", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("+10"))
        .stdout(predicate::str::contains("1"));
}

#[tokio::test]
async fn orders_cancel_all_coin_filter_accepts_spot_pair() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server_with_spot_meta(
        None,
        vec![
            basic_order("@1035", 555, None),
            basic_order("BTC", 111, None),
        ],
        vec![serde_json::json!("success")],
        "cancel",
        spot_meta_with_hype_pair(),
    )
    .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains("\"a\":11035"))
        .and(body_string_contains("\"o\":555"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": "cancel",
                "data": {
                    "statuses": ["success"]
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
            "cancel-all",
            "--coin",
            "HYPE/USDC",
            "-y",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("HYPE/USDC"))
        .stdout(predicate::str::contains("1"));
}

#[tokio::test]
async fn orders_modify_price_and_size_outputs_updated_details() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        Some(basic_order("BTC", 12345, None)),
        vec![],
        vec![serde_json::json!({
            "resting": {
                "oid": 12345,
                "cloid": null
            }
        })],
        "order",
    )
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "modify",
            "12345",
            "--price",
            "51000",
            "--size",
            "0.05",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("modified"))
        .stdout(predicate::str::contains("51000"))
        .stdout(predicate::str::contains("0.05"));
}

#[tokio::test]
async fn orders_modify_prefers_replacement_oid_returned_by_exchange() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server_with_spot_meta(
        Some(basic_order("HYPE/USDC", 12345, None)),
        vec![],
        vec![serde_json::json!({
            "resting": {
                "oid": 67890,
                "cloid": null
            }
        })],
        "order",
        spot_meta_with_hype_pair(),
    )
    .await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "modify",
            "12345",
            "--price",
            "0.3",
            "--size",
            "5",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json[0]["status"], "modified");
    assert_eq!(json[0]["order_id"], 67890);
}

#[tokio::test]
async fn orders_modify_by_cloid_sends_cloid_identifier() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        Some(basic_order("BTC", 12346, Some(TEST_CLOID))),
        vec![],
        vec![serde_json::json!({
            "resting": {
                "oid": 12346,
                "cloid": TEST_CLOID
            }
        })],
        "order",
    )
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "modify",
            "--cloid",
            TEST_CLOID,
            "--price",
            "51000",
            "--size",
            "0.05",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("modified"))
        .stdout(predicate::str::contains(TEST_CLOID));

    let requests = server.received_requests().await.unwrap();
    let exchange_request = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected modify exchange request");
    let body: serde_json::Value = serde_json::from_slice(&exchange_request.body).unwrap();
    assert_eq!(body["action"]["modifies"][0]["oid"], TEST_CLOID);
    assert_eq!(body["action"]["modifies"][0]["order"]["p"], "51000");
    assert_eq!(body["action"]["modifies"][0]["order"]["s"], "0.05");
}

#[test]
fn orders_modify_dry_run_accepts_cloid_identifier() {
    let env = IsolatedHome::new();
    let output = env
        .command()
        .args([
            "--format",
            "json",
            "--dry-run",
            "orders",
            "modify",
            "--cloid",
            TEST_CLOID,
            "--price",
            "51000",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["dry_run"], true);
    assert!(json["args"]["order_id"].is_null());
    assert_eq!(json["args"]["cloid"], TEST_CLOID);
    assert_eq!(json["args"]["price"], "51000");
}

#[test]
fn orders_modify_rejects_oid_and_cloid() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "orders",
            "modify",
            "12345",
            "--cloid",
            TEST_CLOID,
            "--price",
            "51000",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn orders_modify_invalid_cloid_exits_before_auth() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "orders",
            "modify",
            "--cloid",
            "0xabc",
            "--price",
            "51000",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("CLOID must be exactly 16 bytes"))
        .stderr(predicate::str::contains("Authentication required").not());
}

#[tokio::test]
async fn orders_modify_trigger_limit_requires_trigger_price_to_preserve_trigger() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        Some(basic_order_with_type("BTC", 12345, None, "Stop Limit")),
        vec![],
        vec![],
        "order",
    )
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "modify", "12345", "--price", "51000", "--testnet"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--trigger-price"))
        .stderr(predicate::str::contains("stop-limit"));
}

#[tokio::test]
async fn orders_modify_trigger_limit_sends_distinct_limit_and_trigger_prices() {
    let env = IsolatedHome::new();
    let server = mock_order_management_server(
        Some(basic_order_with_type("BTC", 12345, None, "Stop Limit")),
        vec![],
        vec![serde_json::json!({
            "resting": {
                "oid": 12345,
                "cloid": null
            }
        })],
        "order",
    )
    .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "modify",
            "12345",
            "--price",
            "51000",
            "--trigger-price",
            "49000",
            "--size",
            "0.05",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("modified"));

    let requests = server.received_requests().await.unwrap();
    let exchange_request = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected modify exchange request");
    let body: serde_json::Value = serde_json::from_slice(&exchange_request.body).unwrap();
    let order = &body["action"]["modifies"][0]["order"];
    assert_eq!(order["p"], "51000");
    assert_eq!(order["s"], "0.05");
    assert_eq!(order["t"]["trigger"]["isMarket"], false);
    assert_eq!(order["t"]["trigger"]["triggerPx"], "49000");
    assert_eq!(order["t"]["trigger"]["tpsl"], "sl");
}

#[test]
fn orders_modify_requires_price_or_size() {
    let env = IsolatedHome::new();
    env.command()
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "modify", "12345", "--testnet"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "--price, --trigger-price, or --size",
        ));
}
