mod support;

use predicates::prelude::*;
use serde_json::Value;
use support::IsolatedHome;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn outcome_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "allMids"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "BTC": "50000"
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
            "universe": [],
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
        .and(body_partial_json(serde_json::json!({"type": "outcomeMeta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "outcomes": [
                {
                    "outcome": 1,
                    "name": "Recurring BTC",
                    "description": "class:priceBinary|underlying:BTC|expiry:20260510-0000|targetPrice:100000|period:1d",
                    "sideSpecs": [
                        {"name": "Yes", "token": 10},
                        {"name": "No", "token": 11}
                    ]
                },
                {
                    "outcome": 2,
                    "name": "Recurring ETH",
                    "description": "class:priceBinary|underlying:ETH|expiry:20260510-0000|targetPrice:5000|period:1d",
                    "sideSpecs": [
                        {"name": "Yes", "token": 20},
                        {"name": "No", "token": 21}
                    ]
                }
            ],
            "questions": []
        })))
        .mount(&server)
        .await;
    server
}

fn json_stdout(output: Vec<u8>) -> Value {
    serde_json::from_slice(&output).expect("stdout should be valid JSON")
}

#[tokio::test]
async fn outcomes_list_returns_encoded_side_rows() {
    let env = IsolatedHome::new();
    let server = outcome_server().await;

    let out = env
        .command_with_server(&server)
        .args(["--format", "json", "outcomes", "list", "--limit", "2"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    let rows = json.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["outcome"], 1);
    assert_eq!(rows[0]["side"], 0);
    assert_eq!(rows[0]["encoding"], 10);
    assert_eq!(rows[0]["coin"], "#10");
    assert_eq!(rows[0]["token"], "+10");
    assert_eq!(rows[0]["asset_id"], 100000010);
    assert_eq!(rows[0]["outcome_name"], "Recurring BTC");
    assert_eq!(rows[0]["side_name"], "Yes");
}

#[tokio::test]
async fn outcomes_get_accepts_coin_and_token_notation() {
    let env = IsolatedHome::new();
    let server = outcome_server().await;

    for notation in ["#10", "+10"] {
        let out = env
            .command_with_server(&server)
            .args(["--format", "json", "outcomes", "get", notation])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();

        let json = json_stdout(out);
        assert_eq!(json["outcome"], 1);
        assert_eq!(json["side"], 0);
        assert_eq!(json["encoding"], 10);
        assert_eq!(json["coin"], "#10");
        assert_eq!(json["token"], "+10");
    }
}

#[test]
fn outcomes_get_rejects_malformed_notation_before_api_call() {
    let env = IsolatedHome::new();

    env.command()
        .args(["outcomes", "get", "#abc"])
        .assert()
        .code(13)
        .stderr(predicate::str::contains(
            "expected #<encoding> or +<encoding>",
        ));
}

#[tokio::test]
async fn orders_live_outcome_trading_submits_encoded_asset_id() {
    let env = IsolatedHome::new();
    let server = outcome_server().await;
    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_partial_json(serde_json::json!({
            "action": {
                "type": "order",
                "orders": [{"a": 100000010}]
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {
                "type": "order",
                "data": {"statuses": [{"resting": {"oid": 123}}]}
            }
        })))
        .mount(&server)
        .await;

    let out = env
        .command_with_server(&server)
        .env(support::PRIVATE_KEY_ENV, support::VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "orders",
            "create",
            "--coin",
            "#10",
            "--side",
            "buy",
            "--price",
            "0.5",
            "--size",
            "1",
            "--tif",
            "alo",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    assert_eq!(json[0]["order_id"], 123);
    assert_eq!(json[0]["coin"], "#10");
}

#[tokio::test]
async fn orders_outcome_notation_trims_and_rejects_dex_with_specific_message() {
    let env = IsolatedHome::new();
    let server = outcome_server().await;

    let out = env
        .command_with_server(&server)
        .args([
            "--format",
            "json",
            "--dry-run",
            "orders",
            "create",
            "--coin",
            " #10",
            "--side",
            "buy",
            "--price",
            "0.5",
            "--size",
            "1",
            "--tif",
            "alo",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json = json_stdout(out);
    assert_eq!(json["args"]["resolved_asset"], "#10");

    env.command_with_server(&server)
        .args([
            "--dry-run",
            "orders",
            "create",
            "--dex",
            "foo",
            "--coin",
            "#10",
            "--side",
            "buy",
            "--price",
            "0.5",
            "--size",
            "1",
            "--tif",
            "alo",
        ])
        .assert()
        .code(13)
        .stderr(predicate::str::contains("cannot be used with --dex"));
}

#[tokio::test]
async fn orders_outcome_market_dry_run_is_rejected_until_decimals_are_verified() {
    let env = IsolatedHome::new();
    let server = outcome_server().await;

    env.command_with_server(&server)
        .args([
            "--dry-run",
            "orders",
            "create",
            "--coin",
            "#10",
            "--side",
            "buy",
            "--type",
            "market",
            "--amount",
            "1",
        ])
        .assert()
        .code(13)
        .stderr(predicate::str::contains(
            "outcome size decimals are not verified",
        ));
}

#[test]
fn orders_outcome_dry_run_exposes_verified_encoding() {
    let env = IsolatedHome::new();

    let out = env
        .command()
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
            "--price",
            "0.5",
            "--size",
            "1",
            "--tif",
            "alo",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    assert_eq!(json["args"]["outcome"]["encoding"], 10);
    assert_eq!(json["args"]["outcome"]["asset_id"], 100000010);
    assert!(
        json["args"]["outcome"]["live_submission"]
            .as_str()
            .unwrap()
            .contains("enabled")
    );
}
