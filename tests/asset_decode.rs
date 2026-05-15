mod support;

use serde_json::Value;
use support::IsolatedHome;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn asset_decode_server() -> MockServer {
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
            "universe": [
                {"name": "PURR/USDC", "index": 0, "tokens": [1, 0]}
            ],
            "tokens": [
                {"name": "USDC", "index": 0, "tokenId": "0x0", "szDecimals": 6, "weiDecimals": 6, "evmContract": null},
                {"name": "PURR", "index": 1, "tokenId": "0x1", "szDecimals": 0, "weiDecimals": 0, "evmContract": null}
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "meta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [
                {"name": "BTC", "szDecimals": 5, "maxLeverage": 50, "onlyIsolated": false}
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
        .and(body_partial_json(serde_json::json!({"type": "outcomeMeta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "outcomes": [
                {
                    "outcome": 40,
                    "name": "BTC price binary",
                    "description": "class:priceBinary|underlying:BTC|expiry:20260510-0000|targetPrice:100000|period:1d",
                    "sideSpecs": [
                        {"name": "Yes", "token": 400},
                        {"name": "No", "token": 401}
                    ]
                }
            ],
            "questions": []
        })))
        .mount(&server)
        .await;

    server
}

async fn healthcheck_only_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "allMids"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "BTC": "50000"
        })))
        .mount(&server)
        .await;
    server
}

fn json_stdout(output: Vec<u8>) -> Value {
    serde_json::from_slice(&output).expect("stdout should be valid JSON")
}

#[tokio::test]
async fn asset_decode_resolves_perp_spot_and_outcome_ids() {
    let env = IsolatedHome::new();
    let server = asset_decode_server().await;

    let perp = env
        .command_with_server(&server)
        .args(["--format", "json", "asset", "decode", "0"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let perp = json_stdout(perp);
    assert_eq!(perp["asset_id"], 0);
    assert_eq!(perp["kind"], "perp");
    assert_eq!(perp["lookup_status"], "metadata_found");
    assert_eq!(perp["cli_input"], "BTC");
    assert_eq!(perp["perp_index"], 0);

    let spot = env
        .command_with_server(&server)
        .args(["--format", "json", "asset", "decode", "10000"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let spot = json_stdout(spot);
    assert_eq!(spot["kind"], "spot");
    assert_eq!(spot["lookup_status"], "metadata_found");
    assert_eq!(spot["cli_input"], "PURR/USDC");
    assert_eq!(spot["spot_index"], 0);
    assert_eq!(spot["base"], "PURR");
    assert_eq!(spot["quote"], "USDC");

    let outcome = env
        .command_with_server(&server)
        .args(["--format", "json", "asset", "decode", "100000400"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let outcome = json_stdout(outcome);
    assert_eq!(outcome["kind"], "outcome");
    assert_eq!(outcome["lookup_status"], "metadata_found");
    assert_eq!(outcome["coin"], "#400");
    assert_eq!(outcome["token"], "+400");
    assert_eq!(outcome["outcome"], 40);
    assert_eq!(outcome["side"], 0);
    assert_eq!(outcome["side_name"], "Yes");
    assert_eq!(outcome["market_title"], "BTC above 100000 Yes May 10 00:00");
    assert_eq!(outcome["slug"], "btc-above-100000-yes-may-10-0000");
    assert_eq!(outcome["condition"], "BTC above 100000 at May 10 00:00");
    assert_eq!(outcome["condition_class"], "priceBinary");
    assert_eq!(outcome["underlying"], "BTC");
    assert_eq!(outcome["target_price"], "100000");
    assert_eq!(outcome["expiry"], "20260510-0000");
    assert_eq!(outcome["period"], "1d");
    assert!(
        outcome["description"]
            .as_str()
            .unwrap()
            .contains("underlying:BTC")
    );
}

#[tokio::test]
async fn asset_decode_returns_formula_only_for_unresolved_but_valid_ids() {
    let env = IsolatedHome::new();
    let server = asset_decode_server().await;

    let out = env
        .command_with_server(&server)
        .args(["--format", "json", "asset", "decode", "110035"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    assert_eq!(json["kind"], "hip3_perp");
    assert_eq!(json["lookup_status"], "formula_only");
    assert_eq!(json["cli_input"], Value::Null);
    assert_eq!(json["dex_slot"], 1);
    assert_eq!(json["perp_index"], 35);

    let outcome = env
        .command_with_server(&server)
        .args(["--format", "json", "asset", "decode", "100000410"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let outcome = json_stdout(outcome);
    assert_eq!(outcome["kind"], "outcome");
    assert_eq!(outcome["lookup_status"], "formula_only");
    assert_eq!(outcome["cli_input"], "#410");
    assert_eq!(outcome["outcome"], 41);
    assert_eq!(outcome["side"], 0);
    assert_eq!(outcome["side_name"], "Yes");
}

#[tokio::test]
async fn asset_decode_metadata_failures_degrade_to_formula_only() {
    let env = IsolatedHome::new();
    let server = healthcheck_only_server().await;

    let perp = env
        .command_with_server(&server)
        .args(["--format", "json", "asset", "decode", "0"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let perp = json_stdout(perp);
    assert_eq!(perp["kind"], "perp");
    assert_eq!(perp["lookup_status"], "formula_only");
    assert_eq!(perp["cli_input"], Value::Null);
    assert_eq!(perp["perp_index"], 0);

    let outcome = env
        .command_with_server(&server)
        .args(["--format", "json", "asset", "decode", "100000401"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let outcome = json_stdout(outcome);
    assert_eq!(outcome["kind"], "outcome");
    assert_eq!(outcome["lookup_status"], "formula_only");
    assert_eq!(outcome["cli_input"], "#401");
    assert_eq!(outcome["side"], 1);
    assert_eq!(outcome["side_name"], "No");
}

#[tokio::test]
async fn asset_search_finds_symbols_pairs_and_outcome_slugs() {
    let env = IsolatedHome::new();
    let server = asset_decode_server().await;

    let btc = env
        .command_with_server(&server)
        .args(["--format", "json", "asset", "search", "BTC", "--limit", "1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let btc = json_stdout(btc);
    let rows = btc.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["asset_id"], 0);
    assert_eq!(rows[0]["kind"], "perp");
    assert_eq!(rows[0]["cli_input"], "BTC");

    let spot = env
        .command_with_server(&server)
        .args([
            "--format",
            "json",
            "asset",
            "search",
            "PURR/USDC",
            "--limit",
            "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let spot = json_stdout(spot);
    let rows = spot.as_array().unwrap();
    assert_eq!(rows[0]["asset_id"], 10000);
    assert_eq!(rows[0]["kind"], "spot");
    assert_eq!(rows[0]["cli_input"], "PURR/USDC");

    let outcome = env
        .command_with_server(&server)
        .args([
            "--format",
            "json",
            "asset",
            "search",
            "btc-above-100000-yes-may-10-0000",
            "--limit",
            "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let outcome = json_stdout(outcome);
    let rows = outcome.as_array().unwrap();
    assert_eq!(rows[0]["asset_id"], 100000400);
    assert_eq!(rows[0]["kind"], "outcome");
    assert_eq!(rows[0]["slug"], "btc-above-100000-yes-may-10-0000");
    assert_eq!(rows[0]["cli_input"], "#400");
}

#[tokio::test]
async fn asset_search_numeric_query_falls_back_to_decode() {
    let env = IsolatedHome::new();
    let server = asset_decode_server().await;

    let out = env
        .command_with_server(&server)
        .args([
            "--format", "json", "asset", "search", "110035", "--limit", "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    let rows = json.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["asset_id"], 110035);
    assert_eq!(rows[0]["kind"], "hip3_perp");
    assert_eq!(rows[0]["lookup_status"], "formula_only");
    assert_eq!(rows[0]["dex_slot"], 1);
    assert_eq!(rows[0]["perp_index"], 35);
}

#[tokio::test]
async fn asset_decode_supports_json_projection_for_agents() {
    let env = IsolatedHome::new();
    let server = asset_decode_server().await;

    let out = env
        .command_with_server(&server)
        .args([
            "--format",
            "json",
            "--select",
            "asset_id,kind,cli_input",
            "asset",
            "decode",
            "0",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    assert_eq!(
        json,
        serde_json::json!({
            "asset_id": 0,
            "kind": "perp",
            "cli_input": "BTC"
        })
    );
}

#[tokio::test]
async fn asset_decode_invalid_outcome_side_is_structured_not_a_hard_failure() {
    let env = IsolatedHome::new();
    let server = asset_decode_server().await;

    let out = env
        .command_with_server(&server)
        .args(["--format", "json", "asset", "decode", "100000402"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    assert_eq!(json["kind"], "outcome");
    assert_eq!(json["lookup_status"], "invalid_id");
    assert_eq!(json["cli_input"], Value::Null);
    assert_eq!(json["encoding"], 402);
    assert_eq!(json["outcome"], 40);
    assert_eq!(json["side"], 2);
    assert_eq!(
        json["reason"],
        "only binary outcome sides 0 and 1 are supported"
    );
}
