mod support;

use predicates::prelude::*;
use serde_json::Value;
use support::{IsolatedHome, PRIVATE_KEY_ENV, VALID_PRIVATE_KEY, expected_address};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const USER: &str = "0x00000000000000000000000000000000000000aa";
const BUILDER: &str = "0x00000000000000000000000000000000000000bb";
const SECOND_BUILDER: &str = "0x00000000000000000000000000000000000000cc";
async fn builder_server() -> MockServer {
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
        .and(path("/exchange"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {"type": "default"}
        })))
        .mount(&server)
        .await;
    server
}

fn json_stdout(output: Vec<u8>) -> Value {
    serde_json::from_slice(&output).expect("stdout should be valid JSON")
}

#[tokio::test]
async fn builder_max_fee_reads_tenths_bps_and_percent() {
    let env = IsolatedHome::new();
    let server = builder_server().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "maxBuilderFee",
            "user": USER,
            "builder": BUILDER
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(10))
        .mount(&server)
        .await;

    let out = env
        .command_with_server(&server)
        .args([
            "--format",
            "json",
            "builder",
            "max-fee",
            "--user",
            USER,
            "--builder",
            BUILDER,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    assert_eq!(json["user"].as_str().unwrap().to_ascii_lowercase(), USER);
    assert_eq!(
        json["builder"].as_str().unwrap().to_ascii_lowercase(),
        BUILDER
    );
    assert_eq!(json["max_fee_tenths_bps"], 10);
    assert_eq!(json["max_fee_rate"], "0.01%");
}

#[tokio::test]
async fn builder_approved_lists_builders_with_fee_caps() {
    let env = IsolatedHome::new();
    let server = builder_server().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "approvedBuilders",
            "user": USER
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([BUILDER, SECOND_BUILDER])),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "maxBuilderFee",
            "user": USER,
            "builder": BUILDER
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(10))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "maxBuilderFee",
            "user": USER,
            "builder": SECOND_BUILDER
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(1))
        .mount(&server)
        .await;

    let out = env
        .command_with_server(&server)
        .args(["--format", "json", "builder", "approved", "--user", USER])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    let rows = json.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["user"].as_str().unwrap().to_ascii_lowercase(), USER);
    assert_eq!(
        rows[0]["builder"].as_str().unwrap().to_ascii_lowercase(),
        BUILDER
    );
    assert_eq!(rows[0]["max_fee_tenths_bps"], 10);
    assert_eq!(rows[0]["max_fee_rate"], "0.01%");
    assert_eq!(
        rows[1]["builder"].as_str().unwrap().to_ascii_lowercase(),
        SECOND_BUILDER
    );
    assert_eq!(rows[1]["max_fee_tenths_bps"], 1);
    assert_eq!(rows[1]["max_fee_rate"], "0.001%");
}

#[tokio::test]
async fn builder_approved_empty_list_returns_empty_json_array() {
    let env = IsolatedHome::new();
    let server = builder_server().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "approvedBuilders",
            "user": USER
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    let out = env
        .command_with_server(&server)
        .args(["--format", "json", "builder", "approved", "--user", USER])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    assert_eq!(json.as_array().unwrap().len(), 0);
}

#[test]
fn builder_approved_rejects_bad_user_before_network() {
    let env = IsolatedHome::new();

    env.command()
        .env("HYPERLIQUID_FORMAT", "pretty")
        .args(["builder", "approved", "--user", "not-an-address"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "was not found as an address, alias, or id",
        ));
}

#[test]
fn builder_approve_dry_run_previews_action() {
    let env = IsolatedHome::new();
    let signer = expected_address(VALID_PRIVATE_KEY);
    let out = env
        .command()
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "--dry-run",
            "builder",
            "approve",
            "--builder",
            BUILDER,
            "--max-fee-rate",
            "0.001%",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["command"], "builder approve");
    assert_eq!(json["would_execute"], "approve_builder_fee");
    assert_eq!(json["args"]["network"], "Mainnet");
    assert_eq!(json["args"]["signer"], signer);
    assert_eq!(json["args"]["query_address"], signer);
    assert_eq!(json["args"]["max_fee_rate"], "0.001%");
    assert_eq!(json["args"]["max_fee_tenths_bps"], 1);
    assert_eq!(json["args"]["reversibility"], "reversible");
    assert_eq!(json["args"]["action"]["type"], "approveBuilderFee");
    assert_eq!(json["args"]["action"]["maxFeeRate"], "0.001%");
    assert_eq!(
        json["args"]["action"]["builder"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase(),
        BUILDER
    );
    assert_eq!(json["signer"], signer);
    assert_eq!(json["acting_as"], signer);
    assert!(json["vault_address"].is_null());
}

#[test]
fn builder_approve_rejects_bad_fee_before_auth() {
    let env = IsolatedHome::new();

    env.command()
        .env("HYPERLIQUID_FORMAT", "pretty")
        .args([
            "builder",
            "approve",
            "--builder",
            BUILDER,
            "--max-fee-rate",
            "0.0001%",
        ])
        .assert()
        .code(13)
        .stderr(predicate::str::contains("multiple of 0.001%"));
}

#[tokio::test]
async fn builder_approve_submits_signed_payload_with_yes() {
    let env = IsolatedHome::new();
    let server = builder_server().await;
    let user = expected_address(VALID_PRIVATE_KEY);

    let out = env
        .command_with_server(&server)
        .args([
            "--format",
            "json",
            "--private-key",
            VALID_PRIVATE_KEY,
            "builder",
            "approve",
            "--builder",
            BUILDER,
            "--max-fee-rate",
            "0.001%",
            "-y",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    assert_eq!(json["status"], "submitted");
    assert_eq!(json["signer"], user);
    assert_eq!(json["query_address"], user);
    assert_eq!(json["max_fee_tenths_bps"], 1);
    assert_eq!(json["network"], "Mainnet");
    assert_eq!(json["reversibility"], "reversible");

    let requests = server.received_requests().await.unwrap();
    let exchange = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected /exchange request");
    let body: Value = serde_json::from_slice(&exchange.body).unwrap();
    assert_eq!(body["action"]["type"], "approveBuilderFee");
    assert_eq!(body["action"]["maxFeeRate"], "0.001%");
    assert_eq!(
        body["action"]["builder"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase(),
        BUILDER
    );
    assert_eq!(body["action"]["nonce"], body["nonce"]);
    assert!(body["signature"]["r"].as_str().unwrap().starts_with("0x"));
}
