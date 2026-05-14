mod support;

use predicates::prelude::*;
use serde_json::Value;
use support::{
    IsolatedHome, TEST_ACCOUNT_PASSPHRASE, VALID_PRIVATE_KEY, expected_address, mount_all_mids,
};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mock_api_wallet_server() -> MockServer {
    let server = MockServer::start().await;
    mount_all_mids(&server, "50000", "3000").await;
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

#[test]
fn api_wallet_create_dry_run_redacts_generated_private_key() {
    let env = IsolatedHome::new();

    let output = env
        .command()
        .args([
            "--format",
            "json",
            "--dry-run",
            "api-wallet",
            "create",
            "--name",
            "bot-qa",
            "--expires-in",
            "30d",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&output);
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["status"], "dry_run");
    assert_eq!(json["approval_action"]["type"], "approveAgent");
    assert_eq!(json["name"], "bot-qa");
    assert!(
        json["action_agent_name"]
            .as_str()
            .unwrap()
            .contains("valid_until")
    );
    assert!(
        json["approval_action"]["agentAddress"]
            .as_str()
            .unwrap()
            .starts_with("0x")
    );
    assert!(json["expires_at"].as_u64().unwrap() > 0);
    assert!(!stdout.contains("private_key"));
    assert!(!stdout.contains("generated_private_key"));
}

#[test]
fn api_wallet_create_rejects_names_longer_than_protocol_limit_locally() {
    let env = IsolatedHome::new();

    env.command()
        .args(["api-wallet", "create", "--name", "name-longer-than-16"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("between 1 and 16 characters"));
}

#[test]
fn api_wallet_create_counts_unicode_name_length_by_character() {
    let env = IsolatedHome::new();

    env.command()
        .args([
            "--dry-run",
            "api-wallet",
            "create",
            "--name",
            "éééééééééééééééé",
        ])
        .assert()
        .success();

    env.command()
        .args([
            "--dry-run",
            "api-wallet",
            "create",
            "--name",
            "ééééééééééééééééé",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("between 1 and 16 characters"));
}

#[tokio::test]
async fn api_wallet_create_submits_approve_agent_and_prints_generated_key_once() {
    let env = IsolatedHome::new();
    let server = mock_api_wallet_server().await;
    let master = expected_address(VALID_PRIVATE_KEY);

    let output = env
        .command_with_server(&server)
        .args([
            "--format",
            "json",
            "--private-key",
            VALID_PRIVATE_KEY,
            "api-wallet",
            "create",
            "--name",
            "bot-live",
            "--expires-in",
            "1d",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&output);
    let json: Value = serde_json::from_slice(&output).unwrap();
    let private_key = json["private_key"].as_str().unwrap();

    assert_eq!(json["status"], "submitted");
    assert_eq!(json["master_address"], master);
    assert!(private_key.starts_with("0x"));
    assert_eq!(stdout.matches(private_key).count(), 1);

    let requests = server.received_requests().await.unwrap();
    let exchange = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected approveAgent exchange request");
    let body: Value = serde_json::from_slice(&exchange.body).unwrap();
    assert_eq!(body["action"]["type"], "approveAgent");
    assert_eq!(body["action"]["hyperliquidChain"], "Mainnet");
    assert_eq!(
        body["action"]["agentName"]
            .as_str()
            .unwrap()
            .starts_with("bot-live valid_until "),
        true
    );
    assert_eq!(body["action"]["nonce"], body["nonce"]);
    assert!(body["signature"]["r"].as_str().unwrap().starts_with("0x"));
    assert!(!String::from_utf8_lossy(&exchange.body).contains(private_key));
}

#[tokio::test]
async fn api_wallet_generated_key_projection_flags_fail_before_approval() {
    for projection_args in [
        vec!["--select", "status"],
        vec!["--results-only"],
        vec!["--max-results", "1"],
    ] {
        let env = IsolatedHome::new();
        let server = mock_api_wallet_server().await;
        let mut args = vec!["--format", "json"];
        args.extend(projection_args);
        args.extend([
            "--private-key",
            VALID_PRIVATE_KEY,
            "api-wallet",
            "create",
            "--name",
            "bot-live",
        ]);

        let output = env
            .command_with_server(&server)
            .args(args)
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let json: Value = serde_json::from_slice(&output).unwrap();
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .contains("private key exactly once")
        );

        let requests = server.received_requests().await.unwrap();
        assert!(
            requests
                .iter()
                .all(|request| request.url.path() != "/exchange"),
            "projection guard must fail before approveAgent is submitted"
        );
    }
}

#[tokio::test]
async fn api_wallet_list_uses_selected_master_address_for_agent_lookup() {
    let env = IsolatedHome::new();
    let server = mock_api_wallet_server().await;
    let master = expected_address(VALID_PRIVATE_KEY);

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args([
            "--private-key",
            VALID_PRIVATE_KEY,
            "api-wallet",
            "create",
            "--name",
            "stored-bot",
        ])
        .assert()
        .success();

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "extraAgents"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "name": "stored-bot",
                "address": "0x0000000000000000000000000000000000000001",
                "validUntil": 1800000000000_u64
            }
        ])))
        .mount(&server)
        .await;

    let output = env
        .account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args([
            "--format",
            "json",
            "--private-key",
            VALID_PRIVATE_KEY,
            "api-wallet",
            "list",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["master_address"], master);
    assert_eq!(json["api_wallets"][0]["name"], "stored-bot");

    let requests = server.received_requests().await.unwrap();
    let extra_agents = requests
        .iter()
        .find_map(|request| {
            if request.url.path() != "/info" {
                return None;
            }
            let body: Value = serde_json::from_slice(&request.body).ok()?;
            (body["type"] == "extraAgents").then_some(body)
        })
        .expect("expected extraAgents request");
    assert_eq!(
        extra_agents["user"].as_str().unwrap().to_ascii_lowercase(),
        master.to_ascii_lowercase()
    );
}

#[test]
fn api_wallet_revoke_requires_name_in_cli_and_schema() {
    let env = IsolatedHome::new();

    env.command()
        .env("HYPERLIQUID_FORMAT", "pretty")
        .args(["api-wallet", "revoke"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--name"));

    let output = env
        .command()
        .args(["--format", "json", "schema", "api-wallet", "revoke"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["args"][0]["required"], true);
    let required = json["json_schema"]["required"].as_array().unwrap();
    assert!(required.iter().any(|value| value.as_str() == Some("name")));
}

#[tokio::test]
async fn api_wallet_revoke_submits_short_lived_replacement_payload() {
    let env = IsolatedHome::new();
    let server = mock_api_wallet_server().await;

    env.command_with_server(&server)
        .args([
            "--format",
            "json",
            "--private-key",
            VALID_PRIVATE_KEY,
            "api-wallet",
            "revoke",
            "--name",
            "bot-qa",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("private_key").not());

    let requests = server.received_requests().await.unwrap();
    let exchange = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected revoke replacement exchange request");
    let body: Value = serde_json::from_slice(&exchange.body).unwrap();
    assert_eq!(body["action"]["type"], "approveAgent");
    assert!(
        body["action"]["agentName"]
            .as_str()
            .unwrap()
            .starts_with("bot-qa valid_until ")
    );
    assert!(
        body["action"]["agentAddress"]
            .as_str()
            .unwrap()
            .starts_with("0x")
    );
}

#[test]
fn api_wallet_revoke_rejects_names_longer_than_protocol_limit_locally() {
    let env = IsolatedHome::new();

    env.command()
        .args(["api-wallet", "revoke", "--name", "name-longer-than-16"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("between 1 and 16 characters"));
}
