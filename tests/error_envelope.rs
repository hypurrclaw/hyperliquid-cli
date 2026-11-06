// Integration tests for the JSON error envelope, human error blocks, and
// exchange failure classification.

mod support;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use support::{
    API_OVERRIDE_ENV, IsolatedHome, PRIVATE_KEY_ENV, VALID_PRIVATE_KEY, mock_market_server,
    mount_override_healthcheck,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn stdout_json(output: &[u8]) -> Value {
    serde_json::from_slice(output).expect("stdout should be one JSON document")
}

async fn exchange_server(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    mount_override_healthcheck(&server).await;
    Mock::given(method("POST"))
        .and(path("/exchange"))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

async fn exchange_hits(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == "/exchange")
        .count()
}

#[tokio::test]
async fn exchange_503_is_unknown_outcome_with_one_post() {
    let env = IsolatedHome::new();
    let server = exchange_server(ResponseTemplate::new(503)).await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["--format", "json", "staking", "deposit", "--amount", "3"])
        .args(["--testnet"])
        .assert()
        .code(12)
        .get_output()
        .stdout
        .clone();

    let json = stdout_json(&output);
    assert_eq!(json["category"], "unavailable");
    assert_eq!(json["exit_code"], 12);
    assert_eq!(json["reason"], "unknown_outcome");
    assert_eq!(json["retryable"], false);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .starts_with("Can't confirm whether the exchange applied this action."),
        "{json}"
    );
    assert_eq!(json["next"][0]["command"], "hyperliquid orders open");
    assert_eq!(json["next"][1]["command"], "hyperliquid account fills");
    assert_eq!(
        exchange_hits(&server).await,
        1,
        "mutations are never retried"
    );
}

#[tokio::test]
async fn exchange_503_human_output_has_recovery_block() {
    let env = IsolatedHome::new();
    let server = exchange_server(ResponseTemplate::new(503)).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["--format", "table", "staking", "deposit", "--amount", "3"])
        .args(["--testnet"])
        .assert()
        .code(12)
        .stderr(predicate::str::contains(
            "Error: Can't confirm whether the exchange applied this action.",
        ))
        .stderr(predicate::str::contains(
            "\n\nCheck open orders and recent fills:\n    hyperliquid orders open\n    hyperliquid account fills",
        ));
    assert_eq!(exchange_hits(&server).await, 1);
}

#[tokio::test]
async fn exchange_rejection_with_network_words_is_not_unavailable() {
    let env = IsolatedHome::new();
    let server = exchange_server(ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "status": "err",
        "response": "Connection pool timeout while matching dns http 503"
    })))
    .await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["--format", "json", "staking", "deposit", "--amount", "3"])
        .args(["--testnet"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();

    let json = stdout_json(&output);
    assert_eq!(json["reason"], "exchange_rejected");
    assert_ne!(json["category"], "unavailable");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("[untrusted remote data]"),
        "{json}"
    );
}

#[tokio::test]
async fn funding_unknown_asset_suggests_one_close_match_in_data() {
    let env = IsolatedHome::new();
    let server = mock_market_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "funding", "BTCC"])
        .assert()
        .code(13)
        .get_output()
        .stdout
        .clone();

    let json = stdout_json(&output);
    assert_eq!(json["category"], "unsupported");
    assert_eq!(json["exit_code"], 13);
    assert_eq!(json["reason"], "asset_not_found");
    assert_eq!(
        json["error"],
        "Couldn't find asset \"BTCC\". Did you mean BTC?"
    );
    assert_eq!(json["data"]["suggestion"], "BTC");
    assert!(
        json.get("next").is_none(),
        "a suggestion is never a command"
    );
}

#[tokio::test]
async fn funding_unknown_asset_without_close_match_has_no_suggestion() {
    let env = IsolatedHome::new();
    let server = mock_market_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "funding", "NOTACOIN"])
        .assert()
        .code(13)
        .get_output()
        .stdout
        .clone();

    let json = stdout_json(&output);
    assert_eq!(json["reason"], "asset_not_found");
    assert_eq!(json["error"], "Couldn't find asset \"NOTACOIN\".");
    assert!(json.get("data").is_none());
}

#[test]
fn usage_error_envelope_moves_usage_block_into_data() {
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["--format", "json", "wallet", "show", "--bogus"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();

    let json = stdout_json(&output);
    assert_eq!(json["category"], "usage");
    assert_eq!(json["exit_code"], 2);
    assert_eq!(json["reason"], "invalid_arguments");
    let error = json["error"].as_str().unwrap();
    assert!(
        error.starts_with("unexpected argument '--bogus' found"),
        "{error}"
    );
    assert!(!error.contains("Usage:"), "{error}");
    let usage = json["data"]["usage"].as_str().unwrap();
    assert!(usage.starts_with("Usage: "), "{usage}");
    assert!(!usage.contains("See all options"), "{usage}");
    assert_eq!(json["next"][0]["command"], "hyperliquid wallet show --help");
}

#[test]
fn unknown_subcommand_envelope_has_reason_and_suggestion() {
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["--format", "json", "ordres"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();

    let json = stdout_json(&output);
    assert_eq!(json["reason"], "unknown_command");
    assert_eq!(json["data"]["suggestion"], "orders");
}

#[test]
fn auth_required_prints_recovery_block_in_human_mode() {
    let env = IsolatedHome::new();

    env.command()
        .args(["--format", "table", "staking", "deposit", "--amount", "3"])
        .args(["--testnet"])
        .assert()
        .code(10)
        .stderr(predicate::str::starts_with("Error: Authentication required."))
        .stderr(predicate::str::contains(
            "\n\nSet up a wallet:\n    hyperliquid setup\nOr set HYPERLIQUID_PRIVATE_KEY for this run.",
        ));
}

#[test]
fn auth_required_json_envelope_has_next_command() {
    let env = IsolatedHome::new();

    let output = env
        .command()
        .args(["--format", "json", "staking", "deposit", "--amount", "3"])
        .args(["--testnet"])
        .assert()
        .code(10)
        .get_output()
        .stdout
        .clone();

    let json = stdout_json(&output);
    assert_eq!(json["reason"], "auth_required");
    assert_eq!(json["category"], "auth");
    assert_eq!(json["next"][0]["command"], "hyperliquid setup");
}
