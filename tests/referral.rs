use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const API_OVERRIDE_ENV: &str = "HYPERLIQUID_API_BASE_URL";
const PRIVATE_KEY_ENV: &str = "HYPERLIQUID_PRIVATE_KEY";
const TEST_PRIVATE_KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000009";

fn hyperliquid_command() -> Command {
    let mut command = Command::cargo_bin("hyperliquid").unwrap();
    command
        .env("HYPERLIQUID_FORMAT", "pretty")
        .env_remove("HYPERLIQUID_NETWORK")
        .env_remove(API_OVERRIDE_ENV)
        .env_remove("HYPERLIQUID_DEFAULT_REFERRAL_CODE")
        .env(PRIVATE_KEY_ENV, TEST_PRIVATE_KEY);
    command
}

#[test]
fn referral_set_requires_explicit_code_on_mainnet() {
    hyperliquid_command()
        .env("HYPERLIQUID_NETWORK", "mainnet")
        .args(["referral", "set"])
        .assert()
        .code(13)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("referral code is required"));
}

#[tokio::test]
async fn referral_status_outputs_code_and_count() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "referredBy": {
                "referrer": "0x0000000000000000000000000000000000000001",
                "code": "REFERRED"
            },
            "cumVlm": "1000.0",
            "unclaimedRewards": "1.25",
            "claimedRewards": "2.50",
            "builderRewards": "0.10",
            "referrerState": {
                "stage": "ready",
                "data": {
                    "code": "MYCODE",
                    "referralStates": [
                        { "user": "0x0000000000000000000000000000000000000002" },
                        { "user": "0x0000000000000000000000000000000000000003" }
                    ]
                }
            }
        })))
        .mount(&server)
        .await;

    let output = hyperliquid_command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "referral", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["referral_code"], "MYCODE");
    assert_eq!(json["referral_count"], 2);
    assert_eq!(json["referred_by_code"], "REFERRED");
    assert_eq!(json["unclaimed_rewards"], "1.25");
}

#[tokio::test]
async fn referral_status_success_body_with_rate_limit_text_still_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "referredBy": {
                "referrer": "0x0000000000000000000000000000000000000001",
                "code": "RATE-LIMIT-DOCS"
            },
            "referrerState": {
                "stage": "rate-limit-aware-success",
                "data": {
                    "code": "MYCODE",
                    "referralStates": []
                }
            }
        })))
        .mount(&server)
        .await;

    let output = hyperliquid_command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "referral", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["referral_code"], "MYCODE");
    assert_eq!(json["referred_by_code"], "RATE-LIMIT-DOCS");
    assert_eq!(json["referral_count"], 0);
}

#[tokio::test]
async fn referral_set_testnet_defaults_to_testnet_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/exchange"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": { "type": "default" }
        })))
        .mount(&server)
        .await;

    let output = hyperliquid_command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "referral", "set", "--testnet"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    let signer = json["signer"].as_str().unwrap().to_string();
    assert_eq!(json["action"], "set-referrer");
    assert_eq!(json["code"], "TESTNET");
    assert_eq!(json["network"], "Testnet");
    assert_eq!(json["query_address"], signer);
    assert_eq!(json["reversibility"], "irreversible");

    let requests = server.received_requests().await.unwrap();
    let exchange_request = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected /exchange request");
    let body: Value = serde_json::from_slice(&exchange_request.body).unwrap();
    assert_eq!(body["action"]["type"], "setReferrer");
    assert_eq!(body["action"]["code"], "TESTNET");
}

#[tokio::test]
async fn referral_register_submits_register_referrer_action() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/exchange"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": { "type": "default" }
        })))
        .mount(&server)
        .await;

    hyperliquid_command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "referral",
            "register",
            "MYCODE",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("register-referrer"))
        .stdout(predicate::str::contains("MYCODE"));

    let requests = server.received_requests().await.unwrap();
    let exchange_request = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected /exchange request");
    let body: Value = serde_json::from_slice(&exchange_request.body).unwrap();
    assert_eq!(body["action"]["type"], "registerReferrer");
    assert_eq!(body["action"]["code"], "MYCODE");
}

#[test]
fn referral_register_rejects_codes_over_twenty_chars() {
    hyperliquid_command()
        .args([
            "referral",
            "register",
            "abcdefghijklmnopqrstuvwxyz",
            "--testnet",
        ])
        .assert()
        .code(13)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "referrer code must be 20 characters or fewer",
        ));
}

#[tokio::test]
async fn referral_register_dry_run_previews_action() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;

    let output = hyperliquid_command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "--dry-run",
            "referral",
            "register",
            "MYCODE",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    let signer = json["signer"].as_str().unwrap().to_string();
    assert_eq!(json["command"], "referral register");
    assert_eq!(json["would_execute"], "register_referrer_code");
    assert_eq!(json["args"]["network"], "Testnet");
    assert_eq!(json["args"]["signer"], signer);
    assert_eq!(json["args"]["query_address"], signer);
    assert_eq!(json["args"]["code"], "MYCODE");
    assert_eq!(json["args"]["action"]["type"], "registerReferrer");
    assert_eq!(json["args"]["reversibility"], "irreversible");
    assert_eq!(json["acting_as"], signer);
}

#[tokio::test]
async fn referral_set_exchange_rate_limit_exits_11() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/exchange"))
        .respond_with(
            ResponseTemplate::new(429).set_body_string("Too many requests: rate limit exceeded"),
        )
        .mount(&server)
        .await;

    hyperliquid_command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["referral", "set", "MYCODE", "--testnet"])
        .assert()
        .code(11)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Rate limited"))
        .stderr(predicate::str::contains("Unable to reach").not());
}
